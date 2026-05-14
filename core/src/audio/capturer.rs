use livekit::webrtc::native::audio_resampler::AudioResampler;
use rodio::microphone::{self, Input, MicrophoneBuilder};
use rodio::Source;
use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16000;
const TARGET_CHANNELS: u16 = 1;
pub const SAMPLES_DIVIDER: u32 = 100;

/// Suffix duplicate names with `(2)`, `(3)`, ..., skipping suffixes that collide
/// with existing raw names.
fn assign_ui_names(raw: &[String]) -> Vec<String> {
    let mut taken: HashSet<String> = raw.iter().cloned().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    let mut emitted_count: HashMap<&str, u32> = HashMap::new();
    for name in raw {
        let count = emitted_count.entry(name.as_str()).or_insert(0);
        if *count == 0 {
            out.push(name.clone());
        } else {
            let mut n = *count + 1;
            loop {
                let candidate = format!("{name} ({n})");
                if !taken.contains(&candidate) {
                    taken.insert(candidate.clone());
                    out.push(candidate);
                    break;
                }
                n += 1;
            }
        }
        *count += 1;
    }
    out
}

pub fn list_audio_inputs() -> Vec<socket_lib::AudioDevice> {
    match microphone::available_inputs() {
        Ok(mics) => {
            let raw: Vec<String> = mics.iter().map(|m| m.to_string()).collect();
            let ui = assign_ui_names(&raw);
            ui.into_iter()
                .enumerate()
                .map(|(i, name)| socket_lib::AudioDevice {
                    name,
                    default: i == 0,
                })
                .collect()
        }
        Err(e) => {
            log::error!("Failed to list audio inputs: {e}");
            vec![]
        }
    }
}

#[derive(Debug)]
struct AudioDevice {
    name: String,
    ui_name: String,
    input: Input,
}

/// Accumulates RMS over captured samples and emits mic level at a fixed cadence.
struct LevelEmitter {
    socket: socket_lib::SocketSender,
    sum_sq: f64,
    sample_count: usize,
    last_emit: std::time::Instant,
}

impl LevelEmitter {
    const EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

    fn new(socket: socket_lib::SocketSender) -> Self {
        Self {
            socket,
            sum_sq: 0.0,
            sample_count: 0,
            last_emit: std::time::Instant::now(),
        }
    }

    fn observe(&mut self, sample: f32) {
        self.sum_sq += (sample as f64) * (sample as f64);
        self.sample_count += 1;
    }

    fn emit_if_due(&mut self) {
        if self.last_emit.elapsed() < Self::EMIT_INTERVAL || self.sample_count == 0 {
            return;
        }
        let rms = (self.sum_sq / self.sample_count as f64).sqrt() as f32;
        let level = rms.clamp(0.0, 1.0);
        if let Err(e) = self
            .socket
            .send(socket_lib::Message::MicrophoneAudioLevel(level))
        {
            log::warn!("capturer: failed to send mic level: {e:?}");
        }
        self.sum_sq = 0.0;
        self.sample_count = 0;
        self.last_emit = std::time::Instant::now();
    }
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
    socket: socket_lib::SocketSender,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
}

impl Capturer {
    pub fn new(
        proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
        socket: socket_lib::SocketSender,
    ) -> Self {
        let proxy_for_monitor = proxy.clone();
        Self {
            available_devices: vec![],
            capture_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            sample_tx: None,
            active_device_name: None,
            orphaned_threads: Vec::new(),
            _device_monitor: super::device_monitor::DeviceMonitor::new(
                super::device_monitor::DeviceKind::Input,
                proxy_for_monitor,
            )
            .expect("Failed to start input device monitor"),
            socket,
            proxy,
        }
    }

    pub fn list_sources(&mut self) -> Vec<socket_lib::AudioDevice> {
        let mics = microphone::available_inputs().unwrap_or_default();
        let raw_names: Vec<String> = mics.iter().map(|m| m.to_string()).collect();
        let ui_names = assign_ui_names(&raw_names);
        self.available_devices = mics
            .into_iter()
            .zip(raw_names)
            .zip(ui_names)
            .map(|((input, name), ui_name)| AudioDevice {
                name,
                ui_name,
                input,
            })
            .collect();
        self.available_devices
            .iter()
            .enumerate()
            .map(|(i, dev)| socket_lib::AudioDevice {
                name: dev.ui_name.clone(),
                default: i == 0,
            })
            .collect()
    }

    // Instead of reporting the device name to the frontend we are
    // sending a custom name we create which handles conflicts. For example
    // if we have two mics with name Mic, the first will be Mic and the second will
    // be Mic 2. We are making the assumption that during a session if the mics are not
    // plugged/unplugged will keep their relative order. Then we are using this name to match
    // with our internal vector which has stored the actual mic devices.
    pub fn start_capture(
        &mut self,
        device_name: Option<&str>,
        sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    ) -> Result<u32, String> {
        // Stop any existing capture thread before spawning a new one
        if self.capture_thread.is_some() {
            self.stop_thread();
        }

        let builder = MicrophoneBuilder::new();

        let mut fell_back_to_default = false;
        let chosen: Option<&AudioDevice> = device_name.and_then(|name| {
            self.available_devices
                .iter()
                .find(|d| d.ui_name == name)
                .or_else(|| self.available_devices.iter().find(|d| d.name == name))
        });
        let builder = match chosen {
            Some(device) => match builder.device(device.input.clone()) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("Failed to set device: {}, falling back to default", e);
                    fell_back_to_default = true;
                    builder
                        .default_device()
                        .map_err(|e| format!("Failed to get default device: {}", e))?
                }
            },
            None => {
                if let Some(name) = device_name {
                    log::warn!("Device not found: {}, falling back to default", name);
                    fell_back_to_default = true;
                }
                builder
                    .default_device()
                    .map_err(|e| format!("Failed to get default device: {}", e))?
            }
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

        let num_channels = actual_channels.get() as usize;

        if actual_sample_rate != TARGET_SAMPLE_RATE || actual_channels.get() != TARGET_CHANNELS {
            log::info!(
                "Audio conversion: {}Hz {}ch → {}Hz {}ch",
                actual_sample_rate,
                actual_channels.get(),
                TARGET_SAMPLE_RATE,
                TARGET_CHANNELS,
            );
        }

        // Reset and clone the stop flag
        self.stop_flag.store(false, Ordering::Relaxed);
        let stop_flag = Arc::clone(&self.stop_flag);

        self.active_device_name = if device_name.is_none() || fell_back_to_default {
            use cpal::traits::{DeviceTrait, HostTrait};
            #[allow(deprecated)]
            let cpal_raw = cpal::default_host()
                .default_input_device()
                .and_then(|d| d.name().ok());
            cpal_raw.map(|raw| {
                self.available_devices
                    .iter()
                    .find(|d| d.name == raw)
                    .map(|d| d.ui_name.clone())
                    .unwrap_or(raw)
            })
        } else {
            chosen.map(|d| d.ui_name.clone())
        };
        self.sample_tx = Some(sample_tx.clone());

        let socket_for_level = self.socket.clone();
        let proxy_for_thread = self.proxy.clone();

        let handle = std::thread::spawn(move || {
            let mut mic = mic;
            let mut resampler = AudioResampler::default();
            let mut level = LevelEmitter::new(socket_for_level.clone());
            let mut unexpected_exit = false;

            let in_samples_per_frame = actual_sample_rate / SAMPLES_DIVIDER;
            let samples_needed = in_samples_per_frame as usize * num_channels;
            let mut scratch_i16: Vec<i16> = Vec::with_capacity(samples_needed);

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                scratch_i16.clear();
                for sample in mic.by_ref().take(samples_needed) {
                    let clamped = sample.clamp(-1.0, 1.0);
                    level.observe(clamped);
                    scratch_i16.push((clamped * i16::MAX as f32) as i16);
                }

                if scratch_i16.len() < samples_needed {
                    log::error!(
                        "capturing mic: got {}/{} samples, device likely disconnected",
                        scratch_i16.len(),
                        samples_needed
                    );
                    unexpected_exit = true;
                    break;
                }

                let output = resampler.remix_and_resample(
                    &scratch_i16,
                    in_samples_per_frame,
                    actual_channels.get() as u32,
                    actual_sample_rate,
                    TARGET_CHANNELS as u32,
                    TARGET_SAMPLE_RATE,
                );

                if sample_tx.send(output.to_vec()).is_err() {
                    break;
                }

                level.emit_if_due();
            }

            let _ = socket_for_level.send(socket_lib::Message::MicrophoneAudioLevel(0.0));
            if unexpected_exit {
                log::warn!("capture thread died, notifying main thread");
                let _ = proxy_for_thread.send_event(crate::UserEvent::AudioCaptureError);
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
    pub fn handle_default_device_changed(&mut self, force: bool) {
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
                if force {
                    log::info!("Force-switching mic '{name}' to default");
                    if let Err(e) = self.switch_device(None) {
                        log::error!("Failed to force-switch mic to default: {e}");
                    }
                } else {
                    let available = self.list_sources();
                    if !available.iter().any(|d| d.name == name) {
                        log::info!("Active mic '{name}' removed, switching to default");
                        if let Err(e) = self.switch_device(None) {
                            log::error!("Failed to switch mic to default: {e}");
                        }
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
