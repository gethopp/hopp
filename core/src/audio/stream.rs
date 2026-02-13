use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SizedSample, StreamConfig};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

pub struct Stream {
    _cpal_stream: cpal::Stream,
    is_running: Arc<AtomicBool>,
    sample_rate: u32,
}

impl Stream {
    pub fn new(
        device_id: &str,
        sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    ) -> Result<Self, String> {
        let device = find_device_by_id(device_id)?;
        let supported_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {e}"))?;

        log::info!("Device default config: {:?}", supported_config);

        let sample_format = supported_config.sample_format();
        let num_input_channels = supported_config.channels();
        let sample_rate = supported_config.sample_rate();

        let config = StreamConfig {
            channels: num_input_channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        log::info!("AudioStream::new {:?}", config);

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        let cpal_stream = match sample_format {
            SampleFormat::F32 => create_input_stream::<f32>(
                &device,
                &config,
                sample_tx,
                num_input_channels,
                is_running_clone,
            ),
            SampleFormat::I16 => create_input_stream::<i16>(
                &device,
                &config,
                sample_tx,
                num_input_channels,
                is_running_clone,
            ),
            SampleFormat::U16 => create_input_stream::<u16>(
                &device,
                &config,
                sample_tx,
                num_input_channels,
                is_running_clone,
            ),
            other => {
                return Err(format!("Unsupported sample format: {other:?}"));
            }
        }?;

        if let Err(e) = cpal_stream.play() {
            return Err(format!("Failed to start cpal stream: {e}"));
        }

        log::info!("Audio capture stream started");

        Ok(Self {
            _cpal_stream: cpal_stream,
            is_running,
            sample_rate,
        })
    }

    pub fn stop(&self) {
        log::info!("Audio capture stream stopping");
        self.is_running.store(false, Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop();
        log::info!("Audio capture stream stopped");
    }
}

fn find_device_by_id(id: &str) -> Result<Device, String> {
    let host = cpal::default_host();
    let device_id: cpal::DeviceId = id
        .parse()
        .map_err(|e| format!("Invalid device ID '{id}': {e}"))?;
    host.device_by_id(&device_id)
        .ok_or_else(|| format!("Audio device with ID '{id}' not found"))
}

fn create_input_stream<T: SizedSample + Send + 'static>(
    device: &Device,
    config: &StreamConfig,
    sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    num_input_channels: u16,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if !is_running.load(Ordering::Relaxed) {
                    return;
                }

                let converted: Vec<i16> = data
                    .iter()
                    .step_by(num_input_channels as usize)
                    .map(|sample| convert_sample_to_i16(sample))
                    .collect();

                if let Err(e) = sample_tx.send(converted) {
                    log::warn!("Failed to send audio samples: {e}");
                }
            },
            move |err| {
                log::error!("Audio input stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build input stream: {e}"))?;

    Ok(stream)
}

fn convert_sample_to_i16<T: SizedSample>(sample: &T) -> i16 {
    if std::mem::size_of::<T>() == std::mem::size_of::<f32>() {
        let sample_f32 = unsafe { std::mem::transmute_copy::<T, f32>(sample) };
        (sample_f32.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
    } else if std::mem::size_of::<T>() == std::mem::size_of::<i16>() {
        unsafe { std::mem::transmute_copy::<T, i16>(sample) }
    } else if std::mem::size_of::<T>() == std::mem::size_of::<u16>() {
        let sample_u16 = unsafe { std::mem::transmute_copy::<T, u16>(sample) };
        ((sample_u16 as i32) - (u16::MAX as i32 / 2)) as i16
    } else {
        0
    }
}

pub fn list_devices() -> Vec<socket_lib::AudioDevice> {
    let host = cpal::default_host();
    let devices = match host.input_devices() {
        Ok(d) => d,
        Err(e) => {
            log::error!("Failed to enumerate audio input devices: {e}");
            return vec![];
        }
    };

    devices
        .filter_map(|device| {
            let name = device.description().ok()?.name().to_string();
            let id = device.id().ok()?.to_string();
            Some(socket_lib::AudioDevice { name, id })
        })
        .collect()
}
