use rodio::mixer::Mixer;
use rodio::source::SeekError;
use rodio::Source;
use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_BUFFERED_FRAMES: usize = 10;

#[derive(Clone)]
pub struct RodioMixerHandle {
    mixer: Mixer,
}

impl RodioMixerHandle {
    pub fn new(mixer: Mixer) -> Self {
        Self { mixer }
    }

    pub fn add_source(&self, source: &RodioAudioSource) {
        self.mixer.add(BufferedSource {
            shared: source.buffer.clone(),
            active: source.active.clone(),
            current_frame: Vec::new(),
            frame_pos: 0,
            channels: NonZero::new(source.channels).unwrap(),
            sample_rate: NonZero::new(source.sample_rate).unwrap(),
        });
    }

    pub fn remove_source(&self, source: &RodioAudioSource) {
        source.active.store(false, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct RodioAudioSource {
    sample_rate: u32,
    channels: u16,
    buffer: Arc<Mutex<VecDeque<Vec<i16>>>>,
    active: Arc<AtomicBool>,
}

impl RodioAudioSource {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn push_samples(&self, samples: Vec<i16>) {
        let mut buf = self.buffer.lock().unwrap();
        buf.push_back(samples);
        while buf.len() > MAX_BUFFERED_FRAMES {
            buf.pop_front();
        }
    }
}

/// Internal: rodio Source reading from shared buffer.
/// Silence when empty, None when deactivated.
struct BufferedSource {
    shared: Arc<Mutex<VecDeque<Vec<i16>>>>,
    active: Arc<AtomicBool>,
    current_frame: Vec<i16>,
    frame_pos: usize,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
}

impl Iterator for BufferedSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if !self.active.load(Ordering::Relaxed) {
            return None;
        }

        if self.frame_pos < self.current_frame.len() {
            let sample = self.current_frame[self.frame_pos] as f32 / i16::MAX as f32;
            self.frame_pos += 1;
            return Some(sample);
        }

        let mut frames = self.shared.lock().unwrap();
        if let Some(frame) = frames.pop_front() {
            drop(frames);
            self.current_frame = frame;
            self.frame_pos = 1;
            self.current_frame
                .first()
                .map(|&s| s as f32 / i16::MAX as f32)
                .or(Some(0.0))
        } else {
            Some(0.0)
        }
    }
}

impl Source for BufferedSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> NonZero<u16> {
        self.channels
    }
    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, _: Duration) -> Result<(), SeekError> {
        Err(SeekError::NotSupported {
            underlying_source: "BufferedSource",
        })
    }
}
