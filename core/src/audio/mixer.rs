use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::native::audio_mixer;

const MAX_BUFFERED_FRAMES: usize = 10;

/// Wrapper around LiveKit's native AudioMixer that properly sums
/// multiple audio streams using WebRTC's C++ mixer.
#[derive(Clone)]
pub struct AudioMixerHandle {
    inner: Arc<Mutex<audio_mixer::AudioMixer>>,
    next_ssrc: Arc<AtomicI32>,
}

impl std::fmt::Debug for AudioMixerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioMixerHandle").finish()
    }
}

impl AudioMixerHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(audio_mixer::AudioMixer::new())),
            next_ssrc: Arc::new(AtomicI32::new(1)),
        }
    }

    pub fn add_source(&self, source: AudioSource) {
        let mut mixer = self.inner.lock().unwrap();
        mixer.add_source(source);
    }

    pub fn remove_source(&self, ssrc: i32) {
        let mut mixer = self.inner.lock().unwrap();
        mixer.remove_source(ssrc);
    }

    /// Mix all sources and return the mixed audio samples.
    /// The returned slice is valid until the next call to mix.
    pub fn mix(&self, num_channels: usize) -> Vec<i16> {
        let mut mixer = self.inner.lock().unwrap();
        let samples = mixer.mix(num_channels);
        samples.to_vec()
    }

    pub fn next_ssrc(&self) -> i32 {
        self.next_ssrc.fetch_add(1, Ordering::Relaxed)
    }
}

/// An audio source that feeds frames from a remote participant
/// into the native AudioMixer.
#[derive(Clone)]
pub struct AudioSource {
    ssrc: i32,
    sample_rate: u32,
    num_channels: u32,
    buffer: Arc<Mutex<VecDeque<Vec<i16>>>>,
}

impl AudioSource {
    pub fn new(ssrc: i32, sample_rate: u32, num_channels: u32) -> Self {
        Self {
            ssrc,
            sample_rate,
            num_channels,
            buffer: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn ssrc(&self) -> i32 {
        self.ssrc
    }

    /// Push a received audio frame's data into the buffer.
    pub fn receive(&self, frame: &AudioFrame) {
        let mut buf = self.buffer.lock().unwrap();
        buf.push_back(frame.data.to_vec());
        // Cap buffer to avoid unbounded growth
        while buf.len() > MAX_BUFFERED_FRAMES {
            buf.pop_front();
        }
    }
}

impl audio_mixer::AudioMixerSource for AudioSource {
    fn ssrc(&self) -> i32 {
        self.ssrc
    }

    fn preferred_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn get_audio_frame_with_info(&self, _target_sample_rate: u32) -> Option<AudioFrame<'_>> {
        let mut buf = self.buffer.lock().unwrap();
        let data = buf.pop_front()?;
        let samples_per_channel = data.len() as u32 / self.num_channels;
        Some(AudioFrame {
            data: Cow::Owned(data),
            sample_rate: self.sample_rate,
            num_channels: self.num_channels,
            samples_per_channel,
        })
    }
}
