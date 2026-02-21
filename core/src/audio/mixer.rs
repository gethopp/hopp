use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::queue::{self, SourcesQueueInput};
use std::num::NonZero;
use std::sync::Arc;

#[derive(Clone)]
pub struct MixerHandle {
    mixer: Option<Mixer>,
}

impl std::fmt::Debug for MixerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerHandle").finish()
    }
}

impl MixerHandle {
    pub fn new(mixer: Mixer) -> Self {
        Self { mixer: Some(mixer) }
    }

    /// Create a no-op handle that silently drops all audio.
    pub fn disabled() -> Self {
        Self { mixer: None }
    }

    pub fn add_source(&self, sample_rate: u32, channels: u16) -> AudioSource {
        let (tx, rx) = queue::queue(true);
        if let Some(mixer) = &self.mixer {
            mixer.add(rx);
        }
        AudioSource {
            channels: NonZero::new(channels).unwrap(),
            sample_rate: NonZero::new(sample_rate).unwrap(),
            tx,
        }
    }
}

#[derive(Clone)]
pub struct AudioSource {
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    tx: Arc<SourcesQueueInput>,
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        self.tx.set_keep_alive_if_empty(false);
        self.tx.clear();
    }
}

impl AudioSource {
    pub fn push_samples(&self, samples: &[i16]) {
        let floats: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();
        self.tx
            .append(SamplesBuffer::new(self.channels, self.sample_rate, floats));
    }
}
