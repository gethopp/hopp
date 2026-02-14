use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Simple audio mixer that combines audio from multiple sources into a single buffer.
/// Supports mono audio only.
#[derive(Clone, Debug)]
pub struct Mixer {
    buffer: Arc<Mutex<VecDeque<i16>>>,
    max_buffer_size: usize,
}

impl Mixer {
    /// Creates a new audio mixer.
    ///
    /// # Arguments
    /// * `sample_rate` - Sample rate in Hz (e.g., 48000)
    ///
    /// # Returns
    /// A new Mixer instance with a 1-second buffer capacity
    pub fn new(sample_rate: u32) -> Self {
        // 1 second buffer for mono audio
        let max_buffer_size = sample_rate as usize;

        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(max_buffer_size))),
            max_buffer_size,
        }
    }

    /// Adds audio data to the mixer buffer.
    /// If the buffer exceeds max_buffer_size, oldest samples are dropped.
    ///
    /// # Arguments
    /// * `data` - Audio samples as i16
    pub fn add_audio_data(&self, data: &[i16]) {
        let mut buffer = self.buffer.lock().unwrap();

        // Add new samples
        buffer.extend(data.iter().copied());

        // Cap buffer size by removing oldest samples
        while buffer.len() > self.max_buffer_size {
            buffer.pop_front();
        }
    }

    /// Gets the requested number of samples from the buffer.
    /// If not enough samples are available, fills the rest with silence (zeros).
    ///
    /// # Arguments
    /// * `count` - Number of samples to retrieve
    ///
    /// # Returns
    /// Vector of i16 samples, padded with silence if needed
    pub fn get_samples(&self, count: usize) -> Vec<i16> {
        let mut buffer = self.buffer.lock().unwrap();
        let mut samples = Vec::with_capacity(count);

        // Drain available samples
        for _ in 0..count {
            if let Some(sample) = buffer.pop_front() {
                samples.push(sample);
            } else {
                // Fill remaining with silence
                samples.push(0);
            }
        }

        samples
    }
}
