use livekit::options::TrackPublishOptions;
use livekit::track::{LocalAudioTrack, LocalTrack, TrackSource};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::prelude::{AudioSourceOptions, RtcAudioSource};
use livekit::Room;
use tokio::sync::mpsc;

const AUDIO_NUM_CHANNELS: u32 = 1;
const AUDIO_TRACK_NAME: &str = "microphone";
const AUDIO_QUEUE_SIZE: u32 = 1000; // Buffer up to 100 frames (1 second)

pub struct AudioPublisher {
    audio_track: LocalAudioTrack,
    processing_task: tokio::task::JoinHandle<()>,
}

impl AudioPublisher {
    pub async fn publish(
        room: &Room,
        sample_rate: u32,
        sample_rx: mpsc::UnboundedReceiver<Vec<i16>>,
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

        let processing_task =
            tokio::spawn(process_audio_samples(sample_rx, native_source, sample_rate));

        log::info!(
            "AudioPublisher: audio track published with sample rate {}",
            sample_rate
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

async fn process_audio_samples(
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    audio_source: NativeAudioSource,
    sample_rate: u32,
) {
    let mut buffer = Vec::new();
    let samples_per_10ms = (sample_rate / 100) as usize;

    log::info!(
        "Starting audio processing ({}Hz, 1 channel, {} samples per 10ms)",
        sample_rate,
        samples_per_10ms
    );

    while let Some(audio_data) = rx.recv().await {
        buffer.extend_from_slice(&audio_data);

        // Send 10ms chunks to LiveKit
        while buffer.len() >= samples_per_10ms {
            let chunk: Vec<i16> = buffer.drain(..samples_per_10ms).collect();

            let audio_frame = AudioFrame {
                data: chunk.into(),
                sample_rate,
                num_channels: AUDIO_NUM_CHANNELS,
                samples_per_channel: samples_per_10ms as u32,
            };

            if let Err(e) = audio_source.capture_frame(&audio_frame).await {
                log::error!("Failed to send audio frame to LiveKit: {e}");
            }
        }
    }

    log::info!("Audio processing stopped");
}
