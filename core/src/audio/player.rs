use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::mixer::Mixer;

/// Audio playback for remote participants' audio streams.
/// Uses the default output device and plays audio from the mixer.
pub struct Player {
    _stream: Option<cpal::Stream>,
    is_running: Arc<AtomicBool>,
}

impl Player {
    /// Creates a new audio player that plays audio from the given mixer.
    ///
    /// # Arguments
    /// * `mixer` - The audio mixer to read samples from
    ///
    /// # Returns
    /// A new Player instance on success, or an error string if creation fails
    pub fn new(mixer: Mixer) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default output device found".to_string())?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {e}"))?;

        log::info!("Audio player config: {:?}", supported_config);

        let sample_format = supported_config.sample_format();
        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        let stream = match sample_format {
            SampleFormat::F32 => {
                create_output_stream::<f32>(&device, &config, mixer, channels, is_running_clone)
            }
            SampleFormat::I16 => {
                create_output_stream::<i16>(&device, &config, mixer, channels, is_running_clone)
            }
            SampleFormat::U16 => {
                create_output_stream::<u16>(&device, &config, mixer, channels, is_running_clone)
            }
            other => {
                return Err(format!("Unsupported sample format: {other:?}"));
            }
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

    /// Stops the audio playback stream.
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

fn create_output_stream<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    mixer: Mixer,
    num_output_channels: u16,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if !is_running.load(Ordering::Relaxed) {
                    // Fill with silence when not running
                    for sample in data.iter_mut() {
                        *sample = convert_i16_to_sample::<T>(0);
                    }
                    return;
                }

                // Get mono samples from mixer
                let samples_needed = data.len() / num_output_channels as usize;
                let mono_samples = mixer.get_samples(samples_needed);

                // Convert and duplicate mono to all output channels
                for (i, frame) in data.chunks_mut(num_output_channels as usize).enumerate() {
                    let sample = mono_samples.get(i).copied().unwrap_or(0);
                    let converted = convert_i16_to_sample::<T>(sample);
                    for channel_sample in frame.iter_mut() {
                        *channel_sample = converted;
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

fn convert_i16_to_sample<T: SizedSample>(sample: i16) -> T {
    if std::mem::size_of::<T>() == std::mem::size_of::<f32>() {
        let sample_f32 = sample as f32 / i16::MAX as f32;
        unsafe { std::mem::transmute_copy::<f32, T>(&sample_f32) }
    } else if std::mem::size_of::<T>() == std::mem::size_of::<i16>() {
        unsafe { std::mem::transmute_copy::<i16, T>(&sample) }
    } else if std::mem::size_of::<T>() == std::mem::size_of::<u16>() {
        let sample_u16 = ((sample as i32) + (u16::MAX as i32 / 2)) as u16;
        unsafe { std::mem::transmute_copy::<u16, T>(&sample_u16) }
    } else {
        unsafe { std::mem::transmute_copy::<i16, T>(&0i16) }
    }
}
