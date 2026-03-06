use livekit::webrtc::native::apm::AudioProcessingModule;
use tokio::sync::mpsc;

struct MixSource {
    rx: mpsc::UnboundedReceiver<Vec<i16>>,
    buffer: Vec<i16>,
}

/// Sender side for a mix source. Dropping this closes the channel
/// and the processor auto-prunes it on the next mix cycle.
#[derive(Clone)]
pub struct MixSourceHandle {
    tx: mpsc::UnboundedSender<Vec<i16>>,
}

impl MixSourceHandle {
    pub fn push_samples(&self, samples: &[i16]) {
        let _ = self.tx.send(samples.to_vec());
    }
}

/// Clonable handle to add new sources to the processor from any task.
#[derive(Clone)]
pub struct ProcessorHandle {
    source_tx: mpsc::UnboundedSender<mpsc::UnboundedReceiver<Vec<i16>>>,
}

impl ProcessorHandle {
    pub fn add_source(&self) -> MixSourceHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = self.source_tx.send(rx);
        MixSourceHandle { tx }
    }
}

pub struct AudioProcessor {
    apm: AudioProcessingModule,
    sample_rate: i32,
    num_channels: i32,
    sources: Vec<MixSource>,
    new_source_rx: mpsc::UnboundedReceiver<mpsc::UnboundedReceiver<Vec<i16>>>,
    mixed_chunk: Vec<i16>, // pre-allocated, size = chunk_size, never resized
}

impl AudioProcessor {
    pub fn new(sample_rate: i32, num_channels: i32, chunk_size: usize) -> (Self, ProcessorHandle) {
        let apm = AudioProcessingModule::new(true, true, false, true);
        let (source_tx, new_source_rx) = mpsc::unbounded_channel();

        let processor = Self {
            apm,
            sample_rate,
            num_channels,
            sources: Vec::new(),
            new_source_rx,
            mixed_chunk: vec![0i16; chunk_size],
        };

        let handle = ProcessorHandle { source_tx };
        (processor, handle)
    }

    /// Mix all remote sources, feed to APM as reverse stream.
    /// Call BEFORE process() on each 10ms cycle.
    pub fn mix_and_process_reverse(&mut self) {
        // 1. Accept newly added sources
        while let Ok(rx) = self.new_source_rx.try_recv() {
            self.sources.push(MixSource {
                rx,
                buffer: Vec::new(),
            });
        }

        if self.sources.is_empty() {
            return;
        }

        // 2. Drain each source's channel into its buffer
        for source in &mut self.sources {
            while let Ok(samples) = source.rx.try_recv() {
                source.buffer.extend_from_slice(&samples);
            }
        }

        // 3. Zero out mixed_chunk (reuse existing allocation)
        let chunk_size = self.mixed_chunk.len();
        for sample in self.mixed_chunk.iter_mut() {
            *sample = 0;
        }

        // 4. Sum samples from each source
        for source in &mut self.sources {
            let available = source.buffer.len().min(chunk_size);
            for i in 0..available {
                self.mixed_chunk[i] = self.mixed_chunk[i].saturating_add(source.buffer[i]);
            }
            source.buffer.drain(..available);
        }

        // 5. Feed to APM reverse stream
        if let Err(e) = self.apm.process_reverse_stream(
            &mut self.mixed_chunk,
            self.sample_rate,
            self.num_channels,
        ) {
            log::warn!("APM process_reverse_stream failed: {e}");
        }

        // 6. Prune closed sources with empty buffers
        self.sources
            .retain(|source| !source.rx.is_closed() || !source.buffer.is_empty());
    }

    /// Process mic audio through APM (echo cancellation + noise suppression).
    pub fn process(&mut self, chunk: &mut [i16]) {
        if let Err(e) = self
            .apm
            .process_stream(chunk, self.sample_rate, self.num_channels)
        {
            log::warn!("APM process_stream failed: {e}");
        }
    }

    pub fn set_delay(&mut self, delay_ms: i32) {
        if let Err(e) = self.apm.set_stream_delay_ms(delay_ms) {
            log::warn!("APM set_stream_delay_ms failed: {e}");
        }
    }
}
