#[derive(Debug, Clone)]
pub struct RemoteParticipantInfo {
    name: String,
    muted: bool,
    is_speaking: bool,
}

impl RemoteParticipantInfo {
    pub fn new(name: String, muted: bool, is_speaking: bool) -> Self {
        Self {
            name,
            muted,
            is_speaking,
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
}
