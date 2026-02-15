use std::sync::Arc;
use tokio::sync::mpsc;

use crate::livekit::audio::AudioTrackHandle;
use crate::livekit::video::VideoBufferManager;

pub struct ParticipantInfo {
    name: String,
    muted: bool,
    is_speaking: bool,
    camera_buffers: Option<Arc<VideoBufferManager>>,
    audio_handle: Option<AudioTrackHandle>,
    camera_stop_tx: Option<mpsc::UnboundedSender<()>>,
}

impl std::fmt::Debug for ParticipantInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParticipantInfo")
            .field("name", &self.name)
            .field("muted", &self.muted)
            .field("is_speaking", &self.is_speaking)
            .field(
                "camera_buffers",
                &if self.camera_buffers.is_some() {
                    "Some(VideoBufferManager)"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl ParticipantInfo {
    pub fn new(name: String, muted: bool, is_speaking: bool, create_buffers: bool) -> Self {
        let camera_buffers = if create_buffers {
            Some(Arc::new(VideoBufferManager::new()))
        } else {
            None
        };
        Self {
            name,
            muted,
            is_speaking,
            camera_buffers,
            audio_handle: None,
            camera_stop_tx: None,
        }
    }

    /// Creates a ParticipantInfo from a LiveKit remote participant.
    /// Extracts name, muted state (from audio tracks), and speaking state.
    pub fn from_remote_participant(
        participant: &livekit::participant::RemoteParticipant,
        create_buffers: bool,
    ) -> Self {
        let name = participant.name();
        let is_speaking = participant.is_speaking();

        // Check if any audio track is muted
        let mut muted = false;
        for (_, publication) in participant.track_publications() {
            if publication.kind() == livekit::track::TrackKind::Audio {
                muted = publication.is_muted();
                break;
            }
        }

        Self::new(name.clone(), muted, is_speaking, create_buffers)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn is_speaking(&self) -> bool {
        self.is_speaking
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn set_is_speaking(&mut self, is_speaking: bool) {
        self.is_speaking = is_speaking;
    }

    pub fn camera_buffers(&self) -> Option<Arc<VideoBufferManager>> {
        self.camera_buffers.clone()
    }

    pub fn set_camera_buffers(&mut self, buffers: Arc<VideoBufferManager>) {
        self.camera_buffers = Some(buffers);
    }

    pub fn clear_camera_buffers(&mut self) {
        self.camera_buffers = None;
    }

    pub fn set_audio_handle(&mut self, handle: AudioTrackHandle) {
        self.audio_handle = Some(handle);
    }

    pub fn set_camera_stop_tx(&mut self, tx: mpsc::UnboundedSender<()>) {
        self.camera_stop_tx = Some(tx);
    }

    pub fn stop_audio_stream(&mut self) {
        // Dropping the handle removes source from mixer and aborts task
        self.audio_handle.take();
    }

    pub fn stop_camera_stream(&mut self) {
        if let Some(tx) = self.camera_stop_tx.take() {
            let _ = tx.send(());
        }
    }
}
