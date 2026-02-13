use tokio::sync::mpsc;

use super::stream::Stream;

pub struct Capturer {
    stream: Option<Stream>,
}

impl Default for Capturer {
    fn default() -> Self {
        Self::new()
    }
}

impl Capturer {
    pub fn new() -> Self {
        Self { stream: None }
    }

    pub fn list_devices() -> Vec<socket_lib::AudioDevice> {
        super::stream::list_devices()
    }

    pub fn start_capture(
        &mut self,
        device_id: &str,
        sample_tx: mpsc::UnboundedSender<Vec<i16>>,
    ) -> Result<u32, String> {
        self.stop_capture();

        log::info!("start_capture: starting audio capture on device '{device_id}'");
        let stream = Stream::new(device_id, sample_tx)?;
        let sample_rate = stream.sample_rate();
        self.stream = Some(stream);
        Ok(sample_rate)
    }

    pub fn sample_rate(&self) -> Option<u32> {
        self.stream.as_ref().map(|s| s.sample_rate())
    }

    pub fn stop_capture(&mut self) {
        if let Some(stream) = self.stream.take() {
            log::info!("stop_capture: stopping audio capture");
            stream.stop();
        }
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        self.stop_capture();
    }
}
