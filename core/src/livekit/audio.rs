use livekit::options::TrackPublishOptions;
use livekit::track::{LocalAudioTrack, LocalTrack, RemoteAudioTrack, TrackSource};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::prelude::{AudioSourceOptions, RtcAudioSource};
use livekit::Room;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::audio::capturer::SAMPLES_DIVIDER;
use crate::audio::mixer::{AudioSource, MixerHandle};
use crate::audio::processor::{AudioProcessor, MixSourceHandle, ProcessorHandle};
use std::sync::{Arc, Mutex};

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
        processor: Arc<Mutex<AudioProcessor>>,
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
    processor: Arc<Mutex<AudioProcessor>>,
) {
    let samples_per_unit = (sample_rate / SAMPLES_DIVIDER) as usize;
    log::info!(
        "Starting audio processing ({}Hz, {} samples per 10ms)",
        sample_rate,
        samples_per_unit
    );

    let mut buffer: Vec<i16> = Vec::new();
    let mut chunk = vec![0i16; samples_per_unit];

    {
        let mut processor = processor.lock().unwrap();
        processor.set_delay(50);
    }

    while let Some(audio_data) = rx.recv().await {
        buffer.extend_from_slice(&audio_data);

        while buffer.len() >= samples_per_unit {
            chunk.copy_from_slice(&buffer[..samples_per_unit]);
            buffer.drain(..samples_per_unit);
            {
                let mut p = processor.lock().unwrap();
                p.mix_and_process_reverse();
                p.process(&mut chunk);
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
    _mix_source: MixSourceHandle,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for AudioTrackHandle {
    fn drop(&mut self) {
        self.task.abort();
        log::info!("AudioTrackHandle dropped");
    }
}

/// Sets up a remote audio track to feed into the rodio mixer and the APM reverse stream.
/// Returns a handle that cleans up automatically on drop.
pub fn play_remote_audio_track(
    track: RemoteAudioTrack,
    mixer: MixerHandle,
    processor_handle: &ProcessorHandle,
    participant_id: &str,
) -> AudioTrackHandle {
    let source = mixer.add_source(LIVEKIT_SAMPLE_RATE, AUDIO_NUM_CHANNELS as u16);
    let source_clone = source.clone();
    let mix_source = processor_handle.add_source();
    let mix_source_task = mix_source.clone();

    let mut stream = livekit::webrtc::audio_stream::native::NativeAudioStream::new(
        track.rtc_track(),
        LIVEKIT_SAMPLE_RATE as i32,
        AUDIO_NUM_CHANNELS as i32,
    );

    let stream_key = participant_id.to_string();
    let task = tokio::spawn(async move {
        log::info!("Starting audio receive loop for {}", stream_key);
        while let Some(frame) = stream.next().await {
            source_clone.push_samples(&frame.data);
            mix_source_task.push_samples(&frame.data);
        }
        log::info!("Audio receive loop ended for {}", stream_key);
    });

    AudioTrackHandle {
        _source: source,
        _mix_source: mix_source,
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
