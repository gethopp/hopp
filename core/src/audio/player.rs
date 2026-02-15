use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use livekit::webrtc::native::audio_resampler::AudioResampler;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::mixer::AudioMixerHandle;
use crate::livekit::audio::{AUDIO_NUM_CHANNELS, LIVEKIT_SAMPLE_RATE};

/// Audio playback that mixes all remote participants via LiveKit's native AudioMixer
/// and resamples to the output device's sample rate.
pub struct Player {
    _stream: Option<cpal::Stream>,
    is_running: Arc<AtomicBool>,
}

impl Player {
    pub fn new(mixer: AudioMixerHandle) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default output device found".to_string())?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {e}"))?;

        log::info!("Audio player config: {:?}", supported_config);

        let sample_format = supported_config.sample_format();
        let device_sample_rate = supported_config.sample_rate();
        let device_channels = supported_config.channels() as u32;

        let config = StreamConfig {
            channels: device_channels as u16,
            sample_rate: device_sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        let stream = match sample_format {
            SampleFormat::F32 => build_stream_f32(
                &device,
                &config,
                mixer,
                device_sample_rate,
                device_channels,
                is_running_clone,
            ),
            SampleFormat::I16 => build_stream_i16(
                &device,
                &config,
                mixer,
                device_sample_rate,
                device_channels,
                is_running_clone,
            ),
            other => Err(format!("Unsupported sample format: {other:?}")),
        }?;

        if let Err(e) = stream.play() {
            return Err(format!("Failed to start audio playback stream: {e}"));
        }

        log::info!("Audio playback stream started");

        Ok(Self {
            _stream: Some(stream),
            is_running,
        })
    }

    pub fn stop(&self) {
        log::info!("Audio playback stream stopping");
        self.is_running.store(false, Ordering::Relaxed);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
        log::info!("Audio playback stream stopped");
    }
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    mixer: AudioMixerHandle,
    device_sample_rate: u32,
    device_channels: u32,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let mut resampler = AudioResampler::default();
    let mut spillover: Vec<i16> = Vec::new();

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                if !is_running.load(Ordering::Relaxed) {
                    data.fill(0);
                    return;
                }

                let mut offset = 0;

                // Drain spillover first
                if !spillover.is_empty() {
                    let n = spillover.len().min(data.len());
                    data[..n].copy_from_slice(&spillover[..n]);
                    spillover.drain(..n);
                    offset = n;
                }

                while offset < data.len() {
                    let mixed = mixer.mix(AUDIO_NUM_CHANNELS as usize);
                    if mixed.is_empty() {
                        data[offset..].fill(0);
                        break;
                    }

                    let resampled = resampler.remix_and_resample(
                        &mixed,
                        mixed.len() as u32 / AUDIO_NUM_CHANNELS,
                        AUDIO_NUM_CHANNELS,
                        LIVEKIT_SAMPLE_RATE,
                        device_channels,
                        device_sample_rate,
                    );

                    let remaining = data.len() - offset;
                    if resampled.len() <= remaining {
                        data[offset..offset + resampled.len()].copy_from_slice(resampled);
                        offset += resampled.len();
                    } else {
                        data[offset..].copy_from_slice(&resampled[..remaining]);
                        spillover.extend_from_slice(&resampled[remaining..]);
                        offset = data.len();
                    }
                }
            },
            move |err| {
                log::error!("Audio output stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    Ok(stream)
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    mixer: AudioMixerHandle,
    device_sample_rate: u32,
    device_channels: u32,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let mut resampler = AudioResampler::default();
    let mut spillover: Vec<i16> = Vec::new();

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if !is_running.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    return;
                }

                let mut offset = 0;

                // Drain spillover first
                if !spillover.is_empty() {
                    let n = spillover.len().min(data.len());
                    for i in 0..n {
                        data[i] = spillover[i] as f32 / i16::MAX as f32;
                    }
                    spillover.drain(..n);
                    offset = n;
                }

                while offset < data.len() {
                    let mixed = mixer.mix(AUDIO_NUM_CHANNELS as usize);
                    if mixed.is_empty() {
                        data[offset..].fill(0.0);
                        break;
                    }

                    let resampled = resampler.remix_and_resample(
                        &mixed,
                        mixed.len() as u32 / AUDIO_NUM_CHANNELS,
                        AUDIO_NUM_CHANNELS,
                        LIVEKIT_SAMPLE_RATE,
                        device_channels,
                        device_sample_rate,
                    );

                    let remaining = data.len() - offset;
                    let to_copy = resampled.len().min(remaining);
                    for i in 0..to_copy {
                        data[offset + i] = resampled[i] as f32 / i16::MAX as f32;
                    }
                    if resampled.len() > remaining {
                        spillover.extend_from_slice(&resampled[remaining..]);
                    }
                    offset += to_copy;
                }
            },
            move |err| {
                log::error!("Audio output stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    Ok(stream)
}
