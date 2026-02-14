use std::sync::Arc;

use crate::livekit::video::VideoBufferManager;

pub struct ParticipantInfo {
    name: String,
    muted: bool,
    is_speaking: bool,
    camera_buffers: Arc<Option<Arc<VideoBufferManager>>>,
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
    pub fn new(name: String, muted: bool, is_speaking: bool, is_local: bool) -> Self {
        let camera_buffers = if is_local {
            Arc::new(Some(Arc::new(VideoBufferManager::new())))
        } else {
            Arc::new(None)
        };
        Self {
            name,
            muted,
            is_speaking,
            camera_buffers,
        }
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

    pub fn camera_buffers(&self) -> Arc<Option<Arc<VideoBufferManager>>> {
        self.camera_buffers.clone()
    }

    pub fn set_camera_buffers(&mut self, buffers: Arc<VideoBufferManager>) {
        self.camera_buffers = Arc::new(Some(buffers));
    }

    pub fn clear_camera_buffers(&mut self) {
        self.camera_buffers = Arc::new(None);
    }
}
