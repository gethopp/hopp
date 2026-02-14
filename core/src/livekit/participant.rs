use std::sync::Arc;

use crate::livekit::video::VideoBufferManager;

#[derive(Debug)]
pub struct RemoteParticipantInfo {
    name: String,
    muted: bool,
    is_speaking: bool,
    camera_buffers: Option<Arc<VideoBufferManager>>,
}

impl RemoteParticipantInfo {
    pub fn new(name: String, muted: bool, is_speaking: bool) -> Self {
        Self {
            name,
            muted,
            is_speaking,
            camera_buffers: None,
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

    pub fn camera_buffers(&self) -> Option<Arc<VideoBufferManager>> {
        self.camera_buffers.clone()
    }

    pub fn set_camera_buffers(&mut self, buffers: Arc<VideoBufferManager>) {
        self.camera_buffers = Some(buffers);
    }

    pub fn clear_camera_buffers(&mut self) {
        self.camera_buffers = None;
    }
}
