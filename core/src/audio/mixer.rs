use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use log::{error, info};
use rodio::buffer::SamplesBuffer;
use rodio::queue::{self, SourcesQueueInput, SourcesQueueOutput};
use rodio::source::UniformSourceIterator;
use std::num::NonZero;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Weak};

use crate::audio::processor::AudioProcessor;

// TODO: use custom errors here

pub type SharedProcessor = Arc<Mutex<AudioProcessor>>;

struct ResampledSource {
    iter: UniformSourceIterator<SourcesQueueOutput>,
    tx: Weak<Mutex<Arc<SourcesQueueInput>>>,
}

struct SourceMeta {
    tx: Weak<Mutex<Arc<SourcesQueueInput>>>,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
}

struct OutputConfig {
    sample_rate: NonZero<u32>,
    channels: NonZero<u16>,
}

struct MixerInner {
    _stream: cpal::Stream,
    source_tx: Sender<ResampledSource>,
    sources: Vec<SourceMeta>,
    output: OutputConfig,
    processor: SharedProcessor,
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

fn open_output_stream(
    pending_rx: mpsc::Receiver<ResampledSource>,
    processor: SharedProcessor,
) -> Result<(cpal::Stream, OutputConfig), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No default output device")?;
    let cfg = device
        .default_output_config()
        .map_err(|e| format!("Failed to get output config: {e}"))?;

    let sample_rate = cfg.sample_rate();
    let channels = cfg.channels();

    let config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    info!(
        "cpal output: {}Hz {}ch on {:?}",
        sample_rate,
        channels,
        device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_default()
    );

    let mut sources: Vec<ResampledSource> = Vec::new();

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                while let Ok(source) = pending_rx.try_recv() {
                    sources.push(source);
                }
                sources.retain(|s| s.tx.strong_count() > 0);
                for sample in data.iter_mut() {
                    let mut acc = 0.0f32;
                    for source in sources.iter_mut() {
                        if let Some(s) = source.iter.next() {
                            acc += s;
                        }
                    }
                    *sample = acc.clamp(-1.0, 1.0);
                }
                if let Ok(mut proc) = processor.try_lock() {
                    proc.process_reverse(data);
                }
            },
            |err| error!("cpal stream error: {err}"),
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start stream: {e}"))?;

    Ok((
        stream,
        OutputConfig {
            sample_rate: NonZero::new(sample_rate).unwrap(),
            channels: NonZero::new(channels).unwrap(),
        },
    ))
}

impl MixerHandle {
    pub fn new() -> Result<(Self, SharedProcessor), String> {
        let (source_tx, pending_rx) = mpsc::channel();

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No default output device")?;
        let cfg = device
            .default_output_config()
            .map_err(|e| format!("Failed to get output config: {e}"))?;

        let processor = Arc::new(Mutex::new(AudioProcessor::new(
            cfg.sample_rate(),
            cfg.channels() as u32,
        )));

        let (stream, output) = open_output_stream(pending_rx, processor.clone())?;

        let handle = Self {
            inner: Arc::new(Mutex::new(MixerInner {
                _stream: stream,
                source_tx,
                sources: Vec::new(),
                output,
                processor: processor.clone(),
            })),
        };

        Ok((handle, processor))
    }

    pub fn sample_rate(&self) -> u32 {
        self.inner.lock().unwrap().output.sample_rate.get()
    }

    pub fn channels(&self) -> u16 {
        self.inner.lock().unwrap().output.channels.get()
    }

    pub fn add_source(&self, sample_rate: u32, channels: u16) -> AudioSource {
        let ch = NonZero::new(channels).unwrap();
        let sr = NonZero::new(sample_rate).unwrap();

        let (tx, rx) = queue::queue(true);
        // Prime queue so USI reads correct metadata on bootstrap
        tx.append(SamplesBuffer::new(ch, sr, vec![0.0f32; channels as usize]));

        let shared_tx = Arc::new(Mutex::new(tx));
        let mut inner = self.inner.lock().unwrap();

        let usi = UniformSourceIterator::new(rx, inner.output.channels, inner.output.sample_rate);
        let _ = inner.source_tx.send(ResampledSource {
            iter: usi,
            tx: Arc::downgrade(&shared_tx),
        });

        inner.sources.push(SourceMeta {
            tx: Arc::downgrade(&shared_tx),
            channels: ch,
            sample_rate: sr,
        });

        AudioSource {
            channels: ch,
            sample_rate: sr,
            tx: shared_tx,
        }
    }

    pub fn reconnect(&self) -> Result<(), String> {
        let (source_tx, pending_rx) = mpsc::channel();
        let mut inner = self.inner.lock().unwrap();
        let (stream, output) = open_output_stream(pending_rx, inner.processor.clone())?;

        inner
            .processor
            .lock()
            .unwrap()
            .update_speaker_config(output.sample_rate.get(), output.channels.get() as u32);

        let mut live = 0usize;
        inner.sources.retain(|meta| {
            let Some(tx_arc) = meta.tx.upgrade() else {
                return false;
            };
            let (new_tx, new_rx) = queue::queue(true);
            new_tx.append(SamplesBuffer::new(
                meta.channels,
                meta.sample_rate,
                vec![0.0f32; meta.channels.get() as usize],
            ));
            let usi = UniformSourceIterator::new(new_rx, output.channels, output.sample_rate);
            *tx_arc.lock().unwrap() = new_tx;
            let _ = source_tx.send(ResampledSource {
                iter: usi,
                tx: Arc::downgrade(&tx_arc),
            });
            live += 1;
            true
        });

        info!("Audio output reconnected ({live} sources)");

        inner._stream = stream;
        inner.source_tx = source_tx;
        inner.output = output;
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
