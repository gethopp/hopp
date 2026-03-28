use livekit::options::{AudioEncoding, TrackPublishOptions};
use livekit::track::{LocalAudioTrack, LocalTrack, RemoteAudioTrack, TrackSource};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::prelude::{AudioSourceOptions, RtcAudioSource};
use livekit::Room;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::audio::mixer::{AudioSource, MixerHandle, SharedProcessor, MIXER_SAMPLE_RATE};

pub const LIVEKIT_SAMPLE_RATE: u32 = 48000;
pub const AUDIO_NUM_CHANNELS: u32 = 1;
const AUDIO_TRACK_NAME: &str = "microphone";
const AUDIO_QUEUE_SIZE: u32 = 100;

pub struct AudioPublisher {
    audio_track: LocalAudioTrack,
    processing_task: tokio::task::JoinHandle<()>,
}

impl AudioPublisher {
    pub(crate) async fn publish(
        room: &Room,
        sample_rate: u32,
        sample_rx: mpsc::UnboundedReceiver<Vec<i16>>,
        processor: SharedProcessor,
    ) -> Result<Self, String> {
        let audio_source_options = AudioSourceOptions {
            echo_cancellation: false,
            noise_suppression: false,
            auto_gain_control: false,
        };
        let native_source = NativeAudioSource::new(
            audio_source_options,
            sample_rate,
            AUDIO_NUM_CHANNELS,
            AUDIO_QUEUE_SIZE,
        );

        let track = LocalAudioTrack::create_audio_track(
            AUDIO_TRACK_NAME,
            RtcAudioSource::Native(native_source.clone()),
        );

        let now = std::time::Instant::now();
        room.local_participant()
            .publish_track(
                LocalTrack::Audio(track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    audio_encoding: Some(AudioEncoding {
                        max_bitrate: 24_000,
                    }),
                    dtx: false,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| format!("Failed to publish audio track: {e}"))?;
        log::info!("audio_publish: too {}", now.elapsed().as_millis());

        let processing_task = tokio::spawn(process_audio_samples(
            sample_rx,
            native_source,
            sample_rate,
            processor,
        ));

        log::info!("AudioPublisher: audio track published ({}Hz)", sample_rate,);

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

async fn process_audio_samples(
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    audio_source: NativeAudioSource,
    sample_rate: u32,
    processor: SharedProcessor,
) {
    assert_eq!(
        sample_rate, MIXER_SAMPLE_RATE,
        "Mic capture sample rate must match APM rate"
    );
    let samples_per_unit = (MIXER_SAMPLE_RATE / 100) as usize;
    let max_buffer_frames = 10;
    let max_buffer_samples = max_buffer_frames * samples_per_unit;

    log::info!(
        "Starting audio processing ({}Hz, {} samples per 10ms, max buffer {}ms)",
        sample_rate,
        samples_per_unit,
        max_buffer_frames * 10,
    );

    let mut buffer: Vec<i16> = Vec::new();
    let mut chunk = vec![0i16; samples_per_unit];

    while let Some(audio_data) = rx.recv().await {
        // Drain all pending messages into buffer
        buffer.extend_from_slice(&audio_data);
        while let Ok(more) = rx.try_recv() {
            buffer.extend_from_slice(&more);
        }

        // Trim oldest frames if over budget, aligned to frame boundary
        if buffer.len() > max_buffer_samples {
            let total_frames = buffer.len() / samples_per_unit;
            let drop_frames = total_frames - max_buffer_frames;
            let drop_samples = drop_frames * samples_per_unit;
            log::warn!(
                "Audio capture: dropping {}ms ({} frames) to cap latency",
                drop_frames * 10,
                drop_frames,
            );
            buffer.drain(..drop_samples);
        }

        // Process all complete frames
        while buffer.len() >= samples_per_unit {
            chunk.copy_from_slice(&buffer[..samples_per_unit]);
            buffer.drain(..samples_per_unit);
            {
                let mut p = processor.lock();
                let _ = p.process_stream(&mut chunk, sample_rate as i32, AUDIO_NUM_CHANNELS as i32);
            }
            capture_frame(&audio_source, &chunk, samples_per_unit, sample_rate).await;
        }
    }

    log::info!("Audio processing stopped");
}

/// Handle for a remote audio track subscription.
/// On drop, removes the source from the mixer and aborts the receive task.
pub struct AudioTrackHandle {
    _source: AudioSource,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for AudioTrackHandle {
    fn drop(&mut self) {
        self.task.abort();
        log::info!("AudioTrackHandle dropped");
    }
}

/// Sets up a remote audio track to feed into the rodio mixer.
/// Returns a handle that cleans up automatically on drop.
pub fn play_remote_audio_track(
    track: RemoteAudioTrack,
    mixer: MixerHandle,
    participant_id: &str,
) -> AudioTrackHandle {
    let source = mixer.add_source(LIVEKIT_SAMPLE_RATE, AUDIO_NUM_CHANNELS as u16);
    let source_clone = source.clone();

    let mut stream = livekit::webrtc::audio_stream::native::NativeAudioStream::new(
        track.rtc_track(),
        LIVEKIT_SAMPLE_RATE as i32,
        AUDIO_NUM_CHANNELS as i32,
    );

    let stream_key = participant_id.to_string();
    let task = tokio::spawn(async move {
        log::info!("Starting audio receive loop for {}", stream_key);
        let mut frame_count: u64 = 0;
        let mut total_samples: u64 = 0;
        let start = std::time::Instant::now();
        let mut last_log = start;

        while let Some(frame) = stream.next().await {
            source_clone.push_samples(&frame.data);
            frame_count += 1;
            total_samples += frame.data.len() as u64;

            let now = std::time::Instant::now();
            if now.duration_since(last_log).as_secs() >= 5 {
                let elapsed = now.duration_since(start).as_secs_f64();
                let expected_secs = total_samples as f64 / LIVEKIT_SAMPLE_RATE as f64;
                let drift_ms = (expected_secs - elapsed) * 1000.0;
                if drift_ms.abs() > 50.0 {
                    log::warn!(
                        "Audio receive [{}]: drift {:.0}ms ({} frames, expected {:.1}s, wall {:.1}s)",
                        stream_key,
                        drift_ms,
                        frame_count,
                        expected_secs,
                        elapsed,
                    );
                }
                last_log = now;
            }
        }
        log::info!("Audio receive loop ended for {}", stream_key);
    });

    AudioTrackHandle {
        _source: source,
        task,
    }
}

async fn capture_frame(
    audio_source: &NativeAudioSource,
    data: &[i16],
    samples_per_channel: usize,
    sample_rate: u32,
) {
    let audio_frame = AudioFrame {
        data: data.into(),
        sample_rate,
        num_channels: AUDIO_NUM_CHANNELS,
        samples_per_channel: samples_per_channel as u32,
    };

    if let Err(e) = audio_source.capture_frame(&audio_frame).await {
        log::error!("Failed to send audio frame to LiveKit: {e}");
    }
}
