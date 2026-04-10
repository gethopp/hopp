use super::mixer::{MixerHandle, SharedProcessor, MIXER_SAMPLE_RATE};
use thiserror::Error;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

#[derive(Error, Debug)]
pub enum PlayerError {
    #[error("Failed to create mixer: {0}")]
    Mixer(String),
    #[error("Failed to start device monitor: {0}")]
    DeviceMonitor(String),
}

struct PlayerInner {
    mixer: MixerHandle,
    processor: SharedProcessor,
    _device_monitor: super::device_monitor::DeviceMonitor,
}

pub struct Player {
    inner: Option<PlayerInner>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

impl Player {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            inner: None,
            event_loop_proxy: proxy,
        }
    }

    #[allow(unused_variables)]
    pub fn start(&mut self) -> Result<(), PlayerError> {
        log::info!("Player::start");
        let (mixer, processor) = MixerHandle::new().map_err(PlayerError::Mixer)?;

        let device_monitor = super::device_monitor::DeviceMonitor::new(
            super::device_monitor::DeviceKind::Output,
            self.event_loop_proxy.clone(),
        )
        .map_err(|e| PlayerError::DeviceMonitor(format!("{e}")))?;

        self.inner = Some(PlayerInner {
            mixer,
            processor,
            _device_monitor: device_monitor,
        });
        Ok(())
    }

    pub fn stop(&mut self) {
        log::info!("Player::stop");
        self.inner = None;
    }

    pub fn mixer(&self) -> Option<&MixerHandle> {
        self.inner.as_ref().map(|i| &i.mixer)
    }

    pub fn processor(&self) -> Option<SharedProcessor> {
        self.inner.as_ref().map(|i| i.processor.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::capturer::Capturer;
    use std::time::Duration;

    #[test]
    fn test_dual_capture_and_playback() {
        let mut capturer1 = Capturer::new();
        let mut capturer2 = Capturer::new();

        let sources = capturer1.list_sources();
        println!("Available audio input devices:");
        for (name, is_default) in &sources {
            println!("  - {} (default: {})", name, is_default);
        }

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

        let event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event()
            .build()
            .unwrap();
        let mut player = Player::new(event_loop.create_proxy());
        player.start().expect("Failed to start player");
        let mixer = player.mixer().unwrap();

        let s1 = mixer.add_source(16000, 1);
        let s2 = mixer.add_source(16000, 1);

        capturer1.start_capture(None, tx1).expect("capture 1");
        capturer2.start_capture(None, tx2).expect("capture 2");

        std::thread::spawn(move || {
            while let Some(samples) = rx1.blocking_recv() {
                s1.push_samples(&samples);
            }
        });
        std::thread::spawn(move || {
            while let Some(samples) = rx2.blocking_recv() {
                s2.push_samples(&samples);
            }
        });

        println!("Playing two simultaneous captures for 5 seconds...");
        std::thread::sleep(Duration::from_secs(5));

        capturer1.stop_capture();
        capturer2.stop_capture();
        std::thread::sleep(Duration::from_millis(500));
    }
}
