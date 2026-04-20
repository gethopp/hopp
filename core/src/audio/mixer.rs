use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::native::apm::AudioProcessingModule;
use livekit::webrtc::native::audio_mixer::{self, AudioMixer};
use livekit::webrtc::native::audio_resampler::AudioResampler;
use log::{error, info};
use parking_lot::Mutex;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::Arc;

pub type SharedProcessor = Arc<Mutex<AudioProcessingModule>>;

pub const MIXER_SAMPLE_RATE: u32 = 16000;
pub const MIXER_NUM_CHANNELS: u32 = 1;

/// Hard cap on per-source buffered frames (10ms each).
///
/// LiveKit's internal receive task runs on the main runtime and bursts frames
/// to us when it's CPU-starved. We absorb bursts with headroom; if we blow
/// past this cap we crash back by dropping from the front.
const HARD_CAP_FRAMES: usize = 80; // 800ms
const TARGET_DELAY: usize = 20;

struct MixerInner {
    _stream: cpal::Stream,
    mixer: Arc<Mutex<AudioMixer>>,
    apm: SharedProcessor,
    next_ssrc: i32,
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

#[derive(Clone)]
pub struct AudioSource {
    ssrc: i32,
    sample_rate: u32,
    num_channels: u32,
    last_mix: Arc<Mutex<Option<std::time::Instant>>>,
    buffer: Arc<Mutex<VecDeque<Vec<i16>>>>,
}

impl AudioSource {
    pub fn push_samples(&self, samples: &[i16]) {
        let mut buffer = self.buffer.lock();
        buffer.push_back(samples.to_vec());
        if buffer.len() > HARD_CAP_FRAMES {
            while buffer.len() > HARD_CAP_FRAMES {
                buffer.pop_front();
            }
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
        let now = std::time::Instant::now();
        let mut last = self.last_mix.lock();
        if let Some(prev) = *last {
            let gap = now.duration_since(prev);
            if gap.as_millis() > 100 {
                log::warn!(
                    "AudioSource: {}ms gap, flushing stale frames",
                    gap.as_millis()
                );
                self.buffer.lock().clear();
            }
        }
        *last = Some(now);
        drop(last);

        let buf = {
            let mut buffer = self.buffer.lock();
            if buffer.len() > TARGET_DELAY {
                let _ = buffer.pop_front();
            }
            buffer.pop_front()?
        };

        Some(AudioFrame {
            data: Cow::Owned(buf),
            sample_rate: self.sample_rate,
            num_channels: self.num_channels,
            samples_per_channel: self.sample_rate / 100,
        })
    }
}

fn open_output_stream(
    mixer: Arc<Mutex<AudioMixer>>,
    apm: SharedProcessor,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No default output device")?;
    let cfg = device
        .default_output_config()
        .map_err(|e| format!("Failed to get output config: {e}"))?;

    let output_sample_rate = cfg.sample_rate();
    let output_channels = cfg.channels();
    let config = StreamConfig {
        channels: output_channels,
        sample_rate: output_sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    info!(
        "cpal output: {}Hz {}ch",
        output_sample_rate, output_channels
    );

    // Buffer draining pattern adapted from Zed's audio playback implementation:
    // https://github.com/zed-industries/zed/blob/main/crates/audio/src/audio.rs
    let mut resampler = AudioResampler::default();
    let mut buf: Vec<f32> = Vec::new();
    let mut reverse_buf: Vec<i16> = Vec::new();

    let stream = device
        .build_output_stream(
            &config,
            move |mut data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                while !data.is_empty() {
                    if data.len() <= buf.len() {
                        let rest = buf.split_off(data.len());
                        data.copy_from_slice(&buf);
                        buf = rest;
                        return;
                    }
                    if !buf.is_empty() {
                        let (prefix, suffix) = data.split_at_mut(buf.len());
                        prefix.copy_from_slice(&buf);
                        buf.clear();
                        data = suffix;
                    }
                    // Mix a new 10ms frame from all sources (mono at MIXER_SAMPLE_RATE)
                    let mut mixer_guard = mixer.lock();
                    let mixed = mixer_guard.mix(MIXER_NUM_CHANNELS as usize);
                    // Feed mono mix to APM reverse stream before resampling to
                    // multi-channel output.
                    {
                        let mut proc = apm.lock();
                        reverse_buf.clear();
                        reverse_buf.extend_from_slice(mixed);
                        let _ = proc.process_reverse_stream(
                            &mut reverse_buf,
                            MIXER_SAMPLE_RATE as i32,
                            MIXER_NUM_CHANNELS as i32,
                        );
                    }
                    let sampled = resampler.remix_and_resample(
                        mixed,
                        MIXER_SAMPLE_RATE / 100,
                        MIXER_NUM_CHANNELS,
                        MIXER_SAMPLE_RATE,
                        output_channels as u32,
                        output_sample_rate,
                    );
                    buf = sampled
                        .iter()
                        .map(|&s| s as f32 / i16::MAX as f32)
                        .collect();
                }
            },
            |err| error!("cpal stream error: {err}"),
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start stream: {e}"))?;

    Ok(stream)
}

impl MixerHandle {
    pub fn new() -> Result<(Self, SharedProcessor), String> {
        let apm = Arc::new(Mutex::new(AudioProcessingModule::new(
            true, true, false, true,
        )));
        let _ = apm.lock().set_stream_delay_ms(50);
        let mixer = Arc::new(Mutex::new(AudioMixer::new()));
        let stream = open_output_stream(mixer.clone(), apm.clone())?;
        let handle = Self {
            inner: Arc::new(Mutex::new(MixerInner {
                _stream: stream,
                mixer,
                apm: apm.clone(),
                next_ssrc: 1,
            })),
        };
        Ok((handle, apm))
    }

    pub fn add_source(&self, sample_rate: u32, channels: u16) -> AudioSource {
        let mut inner = self.inner.lock();
        let ssrc = inner.next_ssrc;
        inner.next_ssrc += 1;
        let source = AudioSource {
            ssrc,
            sample_rate,
            num_channels: channels as u32,
            last_mix: Arc::new(Mutex::new(None)),
            buffer: Arc::new(Mutex::new(VecDeque::new())),
        };
        inner.mixer.lock().add_source(source.clone());
        source
    }

    pub fn reconnect(&self) -> Result<(), String> {
        let mut inner = self.inner.lock();
        let stream = open_output_stream(inner.mixer.clone(), inner.apm.clone())?;
        inner._stream = stream;
        info!("Audio output reconnected");
        Ok(())
    }
}
