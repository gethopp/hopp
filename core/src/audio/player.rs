use super::mixer::{MixerHandle, SharedProcessor};
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
        .map_err(|e| PlayerError::DeviceMonitor(e.to_string()))?;

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
