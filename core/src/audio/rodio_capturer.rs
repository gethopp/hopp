use cpal::Sample;
use rodio::conversions::SampleRateConverter;
use rodio::microphone::{self, Input, MicrophoneBuilder};
use rodio::Source;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16000;
const TARGET_CHANNELS: u16 = 1;

pub struct AudioDevice {
    pub input: Input,
}

pub struct RodioCapturer {
    available_devices: Vec<AudioDevice>,
    capture_thread: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
}

impl RodioCapturer {
    pub fn new() -> Self {
        Self {
            available_devices: vec![],
            capture_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn list_sources(&mut self) -> Vec<String> {
        let devices = match microphone::available_inputs() {
            Ok(mics) => mics
                .into_iter()
                .map(|input| AudioDevice { input })
                .collect(),
            Err(_) => vec![],
        };
        self.available_devices = devices;
        self.available_devices
            .iter()
            .map(|dev| dev.input.to_string())
            .collect()
    }

    pub fn start_capture(
        &mut self,
        device_name: Option<&str>,
        sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    ) -> Result<u32, String> {
        let builder = MicrophoneBuilder::new();

        // Set up the device
        let builder = if let Some(name) = device_name {
            let device = self
                .available_devices
                .iter()
                .find(|dev| dev.input.to_string() == name)
                .ok_or_else(|| format!("Device not found: {}. Call list_sources() first.", name))?;

            builder
                .device(device.input.clone())
                .map_err(|e| format!("Failed to set device: {}", e))?
        } else {
            builder
                .default_device()
                .map_err(|e| format!("Failed to get default device: {}", e))?
        };

        // Try to configure for 16kHz mono, fall back to default if not supported
        let target_sample_rate = NonZero::new(TARGET_SAMPLE_RATE).unwrap();
        let target_channels = NonZero::new(TARGET_CHANNELS).unwrap();

        let mut mic = match builder
            .default_config()
            .and_then(|b| b.try_sample_rate(target_sample_rate))
            .and_then(|b| b.try_channels(target_channels))
        {
            Ok(configured) => {
                log::info!("Hardware supports 16kHz mono, using native config");
                configured
                    .open_stream()
                    .map_err(|e| format!("Failed to open stream: {}", e))?
            }
            Err(_) => {
                log::info!("Hardware doesn't support 16kHz mono, will resample");
                builder
                    .default_config()
                    .map_err(|e| format!("Failed to get default config: {}", e))?
                    .open_stream()
                    .map_err(|e| format!("Failed to open stream: {}", e))?
            }
        };

        let actual_sample_rate = mic.sample_rate().get();
        let actual_channels = mic.channels();

        // Wrap mic in SampleRateConverter if sample rate differs
        let needs_sample_rate_conversion = actual_sample_rate != TARGET_SAMPLE_RATE;
        let needs_channel_conversion = actual_channels.get() != TARGET_CHANNELS;

        if needs_sample_rate_conversion || needs_channel_conversion {
            log::info!(
                "Audio conversion: {}Hz {}ch → {}Hz {}ch",
                actual_sample_rate,
                actual_channels.get(),
                TARGET_SAMPLE_RATE,
                TARGET_CHANNELS
            );
        }

        // Create an enum to handle both resampled and non-resampled cases
        enum MicSource {
            Direct(rodio::microphone::Microphone),
            Resampled(SampleRateConverter<rodio::microphone::Microphone>),
        }

        impl Iterator for MicSource {
            type Item = f32;

            fn next(&mut self) -> Option<Self::Item> {
                match self {
                    MicSource::Direct(m) => m.next(),
                    MicSource::Resampled(r) => r.next(),
                }
            }
        }

        let source = if needs_sample_rate_conversion {
            log::info!(
                "Using rodio SampleRateConverter: {}Hz → {}Hz",
                actual_sample_rate,
                TARGET_SAMPLE_RATE
            );
            MicSource::Resampled(SampleRateConverter::new(
                mic,
                actual_sample_rate.try_into().unwrap(),
                TARGET_SAMPLE_RATE.try_into().unwrap(),
                actual_channels,
            ))
        } else {
            MicSource::Direct(mic)
        };

        let buffer_size = TARGET_SAMPLE_RATE / 100; // 10ms worth of samples at target rate

        // Reset and clone the stop flag
        self.stop_flag.store(false, Ordering::Relaxed);
        let stop_flag = Arc::clone(&self.stop_flag);

        // Spawn the capture thread
        let handle = std::thread::spawn(move || {
            let mut source = source;

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                // Collect samples (already at target sample rate if resampling was needed)
                let sampled: Vec<i16> = source
                    .by_ref()
                    .take(buffer_size as usize * actual_channels.get() as usize)
                    .map(|s: f32| s.to_sample())
                    .collect();

                if sampled.is_empty() {
                    log::error!("capturing mic failed");
                    break;
                }

                // Convert to mono if needed (average channels)
                let output: Vec<i16> = if needs_channel_conversion {
                    sampled
                        .chunks(actual_channels.get() as usize)
                        .map(|chunk| {
                            let sum: i32 = chunk.iter().map(|&s| s as i32).sum();
                            (sum / chunk.len() as i32) as i16
                        })
                        .collect()
                } else {
                    sampled
                };

                if sample_tx.send(output).is_err() {
                    // receiver has dropped or is not consuming
                    break;
                }
            }
        });

        self.capture_thread = Some(handle);
        Ok(TARGET_SAMPLE_RATE)
    }

    pub fn stop_capture(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.capture_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_list_sources() {
        let mut capturer = RodioCapturer::new();
        let sources = capturer.list_sources();
        println!("Available audio input devices:");
        for device in &sources {
            println!("  - {}", device);
        }
        assert!(
            !sources.is_empty(),
            "Expected at least one audio input device"
        );
    }

    #[test]
    fn test_capture_and_playback() {
        use rodio::{buffer::SamplesBuffer, DeviceSinkBuilder};

        let mut capturer = RodioCapturer::new();
        let sources = capturer.list_sources();
        assert!(!sources.is_empty(), "No audio input devices found");

        let (sample_tx, mut sample_rx) = tokio::sync::mpsc::unbounded_channel();

        let sample_rate = capturer
            .start_capture(Some(&sources[1]), sample_tx)
            .expect("Failed to start capture");

        println!("Capturing audio at {}Hz for 3 seconds...", sample_rate);

        let mut all_samples = Vec::new();
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(samples) = sample_rx.try_recv() {
                all_samples.extend_from_slice(&samples);
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        capturer.stop_capture();
        println!("Captured {} samples", all_samples.len());
        assert!(!all_samples.is_empty(), "No samples were captured");

        println!("Playing back captured audio...");
        let sink = DeviceSinkBuilder::open_default_sink().expect("Failed to create default sink");

        let playback_samples: Vec<f32> = all_samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();

        let channels = NonZero::new(1u16).unwrap();
        let sample_rate_nz = NonZero::new(sample_rate).unwrap();
        let source = SamplesBuffer::new(channels, sample_rate_nz, playback_samples);

        sink.mixer().add(source);
        std::thread::sleep(Duration::from_secs(4));
        println!("Playback complete");
    }

    #[test]
    fn test_capture_default_device() {
        let (sample_tx, mut sample_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut capturer = RodioCapturer::new();
        let sample_rate = capturer
            .start_capture(None, sample_tx)
            .expect("Failed to start capture with default device");

        println!("Capturing from default device at {}Hz", sample_rate);
        std::thread::sleep(Duration::from_secs(1));

        let mut sample_count = 0;
        while let Ok(samples) = sample_rx.try_recv() {
            sample_count += samples.len();
        }

        capturer.stop_capture();
        println!("Captured {} samples", sample_count);
        assert!(sample_count > 0, "No samples were captured");
    }

    #[test]
    #[ignore]
    fn test_stop_and_restart() {
        let mut capturer = RodioCapturer::new();
        let sources = capturer.list_sources();
        assert!(!sources.is_empty(), "No audio input devices found");

        let (sample_tx1, mut sample_rx1) = tokio::sync::mpsc::unbounded_channel();
        capturer
            .start_capture(Some(&sources[0]), sample_tx1)
            .expect("Failed to start first capture");
        std::thread::sleep(Duration::from_millis(500));
        capturer.stop_capture();

        let mut count1 = 0;
        while let Ok(samples) = sample_rx1.try_recv() {
            count1 += samples.len();
        }
        println!("First capture: {} samples", count1);

        let (sample_tx2, mut sample_rx2) = tokio::sync::mpsc::unbounded_channel();
        capturer
            .start_capture(Some(&sources[0]), sample_tx2)
            .expect("Failed to start second capture");
        std::thread::sleep(Duration::from_millis(500));
        capturer.stop_capture();

        let mut count2 = 0;
        while let Ok(samples) = sample_rx2.try_recv() {
            count2 += samples.len();
        }
        println!("Second capture: {} samples", count2);

        assert!(count1 > 0, "First capture produced no samples");
        assert!(count2 > 0, "Second capture produced no samples");
    }
}
