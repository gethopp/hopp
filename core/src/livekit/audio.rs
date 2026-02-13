use std::sync::Arc;

use deep_filter::tract::DfTract;
use livekit::options::TrackPublishOptions;
use livekit::track::{LocalAudioTrack, LocalTrack, TrackSource};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::native::apm::AudioProcessingModule;
use livekit::webrtc::prelude::{AudioSourceOptions, RtcAudioSource};
use livekit::Room;
use ndarray::Array2;
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use tokio::sync::mpsc;

pub(crate) struct SendDfTract(pub(crate) DfTract);
unsafe impl Send for SendDfTract {}

pub(crate) type SharedDf = Arc<std::sync::Mutex<Option<SendDfTract>>>;

const LIVEKIT_SAMPLE_RATE: u32 = 48000;
const AUDIO_NUM_CHANNELS: u32 = 1;
const AUDIO_TRACK_NAME: &str = "microphone";
const AUDIO_QUEUE_SIZE: u32 = 1000; // Buffer up to 100 frames (1 second)

pub struct AudioPublisher {
    audio_track: LocalAudioTrack,
    processing_task: tokio::task::JoinHandle<()>,
}

impl AudioPublisher {
    pub(crate) async fn publish(
        room: &Room,
        sample_rate: u32,
        sample_rx: mpsc::UnboundedReceiver<Vec<i16>>,
        df: SharedDf,
    ) -> Result<Self, String> {
        let audio_source_options = AudioSourceOptions {
            echo_cancellation: false,
            noise_suppression: false,
            auto_gain_control: false,
        };
        let native_source = NativeAudioSource::new(
            audio_source_options,
            LIVEKIT_SAMPLE_RATE,
            AUDIO_NUM_CHANNELS,
            AUDIO_QUEUE_SIZE,
        );

        let track = LocalAudioTrack::create_audio_track(
            AUDIO_TRACK_NAME,
            RtcAudioSource::Native(native_source.clone()),
        );

        room.local_participant()
            .publish_track(
                LocalTrack::Audio(track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| format!("Failed to publish audio track: {e}"))?;

        let resampler = if sample_rate != LIVEKIT_SAMPLE_RATE {
            let chunk_size = 1024;
            let resampler = FastFixedIn::<f64>::new(
                LIVEKIT_SAMPLE_RATE as f64 / sample_rate as f64,
                1.0,
                PolynomialDegree::Linear,
                chunk_size,
                1,
            )
            .map_err(|e| format!("Failed to create resampler: {e}"))?;
            log::info!(
                "AudioPublisher: resampling from {}Hz to {}Hz",
                sample_rate,
                LIVEKIT_SAMPLE_RATE
            );
            Some(resampler)
        } else {
            None
        };

        let apm = AudioProcessingModule::new(false, false, false, false);

        let processing_task = tokio::spawn(process_audio_samples(
            sample_rx,
            native_source,
            resampler,
            df,
            apm,
        ));

        log::info!(
            "AudioPublisher: audio track published (input: {}Hz, output: {}Hz)",
            sample_rate,
            LIVEKIT_SAMPLE_RATE
        );

        Ok(Self {
            audio_track: track,
            processing_task,
        })
    }

    pub async fn unpublish(self, room: &Room) {
        self.processing_task.abort();
        let sid = self.audio_track.sid();
        let res = room.local_participant().unpublish_track(&sid).await;
        if let Err(e) = res {
            log::error!("AudioPublisher::unpublish: failed to unpublish track: {e:?}");
        }
        log::info!("AudioPublisher: audio track unpublished");
    }

    pub fn mute(&self) {
        self.audio_track.mute();
    }

    pub fn unmute(&self) {
        self.audio_track.unmute();
    }
}

fn apply_noise_filter(df: &mut DfTract, chunk: &mut [i16], samples_per_10ms: usize) {
    let floats: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32768.0).collect();
    let noisy = Array2::from_shape_vec((1, samples_per_10ms), floats).expect("shape mismatch");
    let mut enhanced = Array2::zeros((1, samples_per_10ms));
    if let Err(e) = df.process(noisy.view(), enhanced.view_mut()) {
        log::warn!("DeepFilterNet processing failed: {e}");
    } else {
        for (i, sample) in chunk.iter_mut().enumerate() {
            *sample = (enhanced[[0, i]] * 32768.0).clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// Drain 10ms chunks from a resampled buffer, given incoming audio data and a resampler.
fn drain_resampled_chunks(
    audio_data: &[i16],
    input_buf: &mut Vec<f64>,
    resampler: &mut FastFixedIn<f64>,
    output_buf: &mut [Vec<f64>],
    livekit_buf: &mut Vec<i16>,
    samples_per_10ms: usize,
) -> Vec<Vec<i16>> {
    for &s in audio_data {
        input_buf.push(s as f64 / i16::MAX as f64);
    }

    let frames_needed = resampler.input_frames_next();
    while input_buf.len() >= frames_needed {
        let input_slice = [&input_buf[..frames_needed]];

        match resampler.process_into_buffer(&input_slice, output_buf, None) {
            Ok((_, out_len)) => {
                livekit_buf.extend(
                    output_buf[0][..out_len]
                        .iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f64) as i16),
                );
            }
            Err(e) => {
                log::warn!("Resampling error: {e}");
            }
        }

        input_buf.drain(..frames_needed);
    }

    let mut chunks = Vec::new();
    while livekit_buf.len() >= samples_per_10ms {
        chunks.push(livekit_buf.drain(..samples_per_10ms).collect());
    }
    chunks
}

/// Drain 10ms chunks from a passthrough buffer.
fn drain_passthrough_chunks(
    audio_data: &[i16],
    buffer: &mut Vec<i16>,
    samples_per_10ms: usize,
) -> Vec<Vec<i16>> {
    buffer.extend_from_slice(audio_data);

    let mut chunks = Vec::new();
    while buffer.len() >= samples_per_10ms {
        chunks.push(buffer.drain(..samples_per_10ms).collect());
    }
    chunks
}

async fn process_audio_samples(
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    audio_source: NativeAudioSource,
    resampler: Option<FastFixedIn<f64>>,
    df: SharedDf,
    mut apm: AudioProcessingModule,
) {
    let samples_per_10ms = (LIVEKIT_SAMPLE_RATE / 100) as usize;

    log::info!(
        "Starting audio processing ({}Hz, 1 channel, {} samples per 10ms)",
        LIVEKIT_SAMPLE_RATE,
        samples_per_10ms
    );

    // Per-branch state
    let mut resampler = resampler;
    let mut input_buf: Vec<f64> = Vec::new();
    let mut output_buf = resampler
        .as_ref()
        .map(|r| vec![vec![0f64; r.output_frames_max()]; 1])
        .unwrap_or_default();
    let mut livekit_buf: Vec<i16> = Vec::new();
    let mut passthrough_buf: Vec<i16> = Vec::new();

    while let Some(audio_data) = rx.recv().await {
        let chunks = if let Some(ref mut r) = resampler {
            drain_resampled_chunks(
                &audio_data,
                &mut input_buf,
                r,
                &mut output_buf,
                &mut livekit_buf,
                samples_per_10ms,
            )
        } else {
            drain_passthrough_chunks(&audio_data, &mut passthrough_buf, samples_per_10ms)
        };

        for mut chunk in chunks {
            if let Err(e) = apm.process_stream(
                &mut chunk,
                LIVEKIT_SAMPLE_RATE as i32,
                AUDIO_NUM_CHANNELS as i32,
            ) {
                log::warn!("APM process_stream failed: {e}");
            }
            {
                let mut guard = df.lock().unwrap();
                if let Some(ref mut model) = *guard {
                    apply_noise_filter(&mut model.0, &mut chunk, samples_per_10ms);
                }
            }
            capture_frame(&audio_source, chunk, samples_per_10ms).await;
        }
    }

    log::info!("Audio processing stopped");
}

async fn capture_frame(
    audio_source: &NativeAudioSource,
    data: Vec<i16>,
    samples_per_channel: usize,
) {
    let audio_frame = AudioFrame {
        data: data.into(),
        sample_rate: LIVEKIT_SAMPLE_RATE,
        num_channels: AUDIO_NUM_CHANNELS,
        samples_per_channel: samples_per_channel as u32,
    };

    if let Err(e) = audio_source.capture_frame(&audio_frame).await {
        log::error!("Failed to send audio frame to LiveKit: {e}");
    }
}
