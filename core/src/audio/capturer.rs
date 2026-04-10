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
pub const SAMPLES_DIVIDER: u32 = 100;

/// List audio input devices (sorted by name). First entry is marked default for UI selection.
/// Does not require a [`Capturer`] instance — mirrors [`crate::camera::capturer::CameraCapturer::list_devices`].
pub fn list_audio_inputs() -> Vec<(String, bool)> {
    match microphone::available_inputs() {
        Ok(mics) => {
            let mut devices: Vec<_> = mics.iter().map(|input| input.to_string()).collect();
            devices.sort();
            devices
                .into_iter()
                .enumerate()
                .map(|(i, name)| (name, i == 0))
                .collect()
        }
        Err(e) => {
            log::error!("Failed to list audio inputs: {e}");
            vec![]
        }
    }
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

struct AudioDevice {
    input: Input,
}

pub struct Capturer {
    available_devices: Vec<AudioDevice>,
    capture_thread: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    sample_tx: Option<mpsc::UnboundedSender<Vec<i16>>>,
    active_device_name: Option<String>,
    /// Threads that were asked to stop but may still be blocked in `source.next()`.
    /// We try-join them on each `stop_thread` call; they'll eventually exit when
    /// the device errors out or the process ends.
    orphaned_threads: Vec<JoinHandle<()>>,
    _device_monitor: super::device_monitor::DeviceMonitor,
}

impl Capturer {
    #[allow(unused_variables)]
    pub fn new(proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>) -> Self {
        Self {
            available_devices: vec![],
            capture_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            sample_tx: None,
            active_device_name: None,
            orphaned_threads: Vec::new(),
            _device_monitor: super::device_monitor::DeviceMonitor::new(
                super::device_monitor::DeviceKind::Input,
                proxy,
            )
            .expect("Failed to start input device monitor"),
        }
    }

    pub fn list_sources(&mut self) -> Vec<(String, bool)> {
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
            .enumerate()
            .map(|(i, dev)| (dev.input.to_string(), i == 0))
            .collect()
    }

    pub fn start_capture(
        &mut self,
        device_name: Option<&str>,
        sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    ) -> Result<u32, String> {
        let builder = MicrophoneBuilder::new();

        // Set up the device
        let mut fell_back_to_default = false;
        let builder = if let Some(name) = device_name {
            let device = self
                .available_devices
                .iter()
                .find(|dev| dev.input.to_string() == name);

            match device {
                Some(device) => match builder.device(device.input.clone()) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("Failed to set device: {}, falling back to default", e);
                        builder
                            .default_device()
                            .map_err(|e| format!("Failed to get default device: {}", e))?
                    }
                },
                None => {
                    log::warn!("Device not found: {}, falling back to default", name);
                    fell_back_to_default = true;
                    builder
                        .default_device()
                        .map_err(|e| format!("Failed to get default device: {}", e))?
                }
            }
        } else {
            builder
                .default_device()
                .map_err(|e| format!("Failed to get default device: {}", e))?
        };

        // Try to configure for 16kHz mono, fall back to default if not supported
        let target_sample_rate = NonZero::new(TARGET_SAMPLE_RATE).unwrap();
        let target_channels = NonZero::new(TARGET_CHANNELS).unwrap();

        let mic = match builder
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

        if needs_sample_rate_conversion || actual_channels.get() != TARGET_CHANNELS {
            log::info!(
                "Audio conversion: {}Hz {}ch → {}Hz {}ch",
                actual_sample_rate,
                actual_channels.get(),
                TARGET_SAMPLE_RATE,
                TARGET_CHANNELS
            );
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

        let buffer_frames = (TARGET_SAMPLE_RATE / SAMPLES_DIVIDER) as usize;
        let num_channels = actual_channels.get() as usize;
        let take_count = buffer_frames * num_channels;

        // Reset and clone the stop flag
        self.stop_flag.store(false, Ordering::Relaxed);
        let stop_flag = Arc::clone(&self.stop_flag);

        self.active_device_name = if device_name.is_none() || fell_back_to_default {
            use cpal::traits::{DeviceTrait, HostTrait};
            cpal::default_host()
                .default_input_device()
                .and_then(|d| d.name().ok())
        } else {
            device_name.map(|s| s.to_string())
        };
        self.sample_tx = Some(sample_tx.clone());

        // Spawn the capture thread
        let handle = std::thread::spawn(move || {
            let mut source = source;
            let mut output = Vec::with_capacity(buffer_frames);

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                output.clear();
                let mut sample_idx = 0;
                let mut got_any = false;
                for s in source.by_ref().take(take_count) {
                    got_any = true;
                    if sample_idx % num_channels == 0 {
                        output.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                    }
                    sample_idx += 1;
                }

                if !got_any {
                    log::error!("capturing mic failed");
                    break;
                }

                if sample_tx
                    .send(std::mem::replace(
                        &mut output,
                        Vec::with_capacity(buffer_frames),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });

        self.capture_thread = Some(handle);
        Ok(TARGET_SAMPLE_RATE)
    }

    pub fn is_capturing(&self) -> bool {
        self.capture_thread.is_some()
    }

    pub fn switch_device(&mut self, device_name: Option<&str>) -> Result<u32, String> {
        let sample_tx = self
            .sample_tx
            .clone()
            .ok_or_else(|| "No active capture to switch".to_string())?;
        self.list_sources();
        self.stop_thread();
        self.start_capture(device_name, sample_tx)
    }

    pub fn active_device_name(&self) -> Option<&str> {
        self.active_device_name.as_deref()
    }

    /// Called when the OS default input device changes.
    /// If using default: reconnect (the default changed).
    /// If using a named device: only switch if that device disappeared.
    pub fn handle_default_device_changed(&mut self) {
        if !self.is_capturing() {
            return;
        }
        match &self.active_device_name {
            None => {
                log::info!("Was using default mic, reconnecting to new default...");
                if let Err(e) = self.switch_device(None) {
                    log::error!("Failed to reconnect mic to new default: {e}");
                }
            }
            Some(name) => {
                let name = name.clone();
                let available = self.list_sources();
                if !available.iter().any(|d| d.0 == name) {
                    log::info!("Active mic '{name}' removed, switching to default");
                    if let Err(e) = self.switch_device(None) {
                        log::error!("Failed to switch mic to default: {e}");
                    }
                }
            }
        }
    }

    fn stop_thread(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.capture_thread.take() {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                log::warn!("Capture thread still running, orphaning it");
                self.orphaned_threads.push(handle);
            }
        }
        // Sweep orphaned threads that have since finished
        let mut still_running = Vec::new();
        for handle in self.orphaned_threads.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
                log::info!("Orphaned capture thread finished");
            } else {
                still_running.push(handle);
            }
        }
        self.orphaned_threads = still_running;
        if !self.orphaned_threads.is_empty() {
            log::warn!(
                "{} orphaned capture thread(s) still running",
                self.orphaned_threads.len()
            );
        }
        // Allocate a fresh stop flag for the next capture thread
        self.stop_flag = Arc::new(AtomicBool::new(false));
    }

    pub fn stop_capture(&mut self) {
        self.stop_thread();
        self.sample_tx = None;
        self.active_device_name = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_list_sources() {
        let mut capturer = Capturer::new();
        let sources = capturer.list_sources();
        println!("Available audio input devices:");
        for (name, is_default) in &sources {
            println!("  - {} (default: {})", name, is_default);
        }
        assert!(
            !sources.is_empty(),
            "Expected at least one audio input device"
        );
    }

    #[test]
    fn test_capture_and_playback() {
        use rodio::{buffer::SamplesBuffer, DeviceSinkBuilder};

        let mut capturer = Capturer::new();
        let sources = capturer.list_sources();
        assert!(!sources.is_empty(), "No audio input devices found");

        let (sample_tx, mut sample_rx) = tokio::sync::mpsc::unbounded_channel();

        let sample_rate = capturer
            .start_capture(Some(&sources[1].0), sample_tx)
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

        let mut capturer = Capturer::new();
        let sources = capturer.list_sources();
        println!("Available audio input devices:");
        for (name, is_default) in &sources {
            println!("  - {} (default: {})", name, is_default);
        }
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
        let mut capturer = Capturer::new();
        let sources = capturer.list_sources();
        assert!(!sources.is_empty(), "No audio input devices found");

        let (sample_tx1, mut sample_rx1) = tokio::sync::mpsc::unbounded_channel();
        capturer
            .start_capture(Some(&sources[0].0), sample_tx1)
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
            .start_capture(Some(&sources[0].0), sample_tx2)
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
