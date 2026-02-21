use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::queue::{self, SourcesQueueInput};
use std::num::NonZero;
use std::sync::Arc;

#[derive(Clone)]
pub struct RodioMixerHandle {
    mixer: Mixer,
}

impl RodioMixerHandle {
    pub fn new(mixer: Mixer) -> Self {
        Self { mixer }
    }

    pub fn add_source(&self, sample_rate: u32, channels: u16) -> RodioAudioSource {
        let (tx, rx) = queue::queue(true);
        self.mixer.add(rx);
        RodioAudioSource {
            channels: NonZero::new(channels).unwrap(),
            sample_rate: NonZero::new(sample_rate).unwrap(),
            tx,
        }
    }
}

pub struct RodioAudioSource {
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    tx: Arc<SourcesQueueInput>,
}

impl Drop for RodioAudioSource {
    fn drop(&mut self) {
        self.tx.set_keep_alive_if_empty(false);
        self.tx.clear();
    }
}

impl RodioAudioSource {
    pub fn push_samples(&self, samples: Vec<i16>) {
        let floats: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();
        self.tx
            .append(SamplesBuffer::new(self.channels, self.sample_rate, floats));
    }
}
