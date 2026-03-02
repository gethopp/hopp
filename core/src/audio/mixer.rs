use log::info;
use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::queue::{self, SourcesQueueInput};
use rodio::source::Zero;
use rodio::{DeviceSinkBuilder, MixerDeviceSink};
use std::num::NonZero;
use std::sync::{Arc, Mutex, Weak};

struct SourceEntry {
    tx: Weak<Mutex<Arc<SourcesQueueInput>>>,
}

struct MixerInner {
    _sink: MixerDeviceSink,
    mixer: Mixer,
    sources: Vec<SourceEntry>,
}

#[derive(Clone)]
pub struct MixerHandle {
    inner: Arc<Mutex<MixerInner>>,
}

impl std::fmt::Debug for MixerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerHandle").finish()
    }
}

fn create_sink_and_mixer() -> Result<(MixerDeviceSink, Mixer), String> {
    let sink_builder = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("Failed to get default output device: {e}"))?;
    let sink_builder = sink_builder.with_error_callback(|e| eprintln!("Audio sink error: {e}"));
    let mut sink = sink_builder
        .open_stream()
        .map_err(|e| format!("Failed to open default sink: {e}"))?;
    sink.log_on_drop(false);

    let rodio_mixer = sink.mixer().clone();

    // Infinite silence keeps mixer attached to the output stream.
    rodio_mixer.add(Zero::new(
        NonZero::new(1u16).unwrap(),
        NonZero::new(16000u32).unwrap(),
    ));

    Ok((sink, rodio_mixer))
}

impl MixerHandle {
    pub fn new() -> Result<Self, String> {
        let (sink, mixer) = create_sink_and_mixer()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(MixerInner {
                _sink: sink,
                mixer,
                sources: Vec::new(),
            })),
        })
    }

    pub fn add_source(&self, sample_rate: u32, channels: u16) -> AudioSource {
        let (tx, rx) = queue::queue(true);
        let shared_tx = Arc::new(Mutex::new(tx));

        let mut inner = self.inner.lock().unwrap();
        inner.mixer.add(rx);
        inner.sources.push(SourceEntry {
            tx: Arc::downgrade(&shared_tx),
        });

        AudioSource {
            channels: NonZero::new(channels).unwrap(),
            sample_rate: NonZero::new(sample_rate).unwrap(),
            tx: shared_tx,
        }
    }

    // TODO: test this doesn't leak anything
    pub fn reconnect(&self) -> Result<(), String> {
        let (sink, mixer) = create_sink_and_mixer()?;

        let mut inner = self.inner.lock().unwrap();

        // Rewire each live source to the new mixer; drop entries whose AudioSource was dropped
        let mut live = 0usize;
        inner.sources.retain(|source| {
            let Some(tx) = source.tx.upgrade() else {
                return false;
            };
            let (new_tx, new_rx) = queue::queue(true);
            mixer.add(new_rx);
            // Old tx drops here → old queue's buffered samples discarded
            *tx.lock().unwrap() = new_tx;
            live += 1;
            true
        });

        info!("Audio output reconnected ({live} sources)");

        inner._sink = sink;
        inner.mixer = mixer;

        Ok(())
    }
}

#[derive(Clone)]
pub struct AudioSource {
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    tx: Arc<Mutex<Arc<SourcesQueueInput>>>,
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        let tx = self.tx.lock().unwrap();
        tx.set_keep_alive_if_empty(false);
        tx.clear();
    }
}

impl AudioSource {
    pub fn push_samples(&self, samples: &[i16]) {
        let floats: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();
        let tx = self.tx.lock().unwrap();
        tx.append(SamplesBuffer::new(self.channels, self.sample_rate, floats));
    }
}
