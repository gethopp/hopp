use super::mixer::MixerHandle;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

pub struct Player {
    mixer: MixerHandle,
    #[cfg(target_os = "macos")]
    _device_monitor: super::device_monitor::DeviceMonitor,
}

impl Player {
    #[allow(unused_variables)]
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let mixer = MixerHandle::new()?;

        #[cfg(target_os = "macos")]
        let device_monitor = super::device_monitor::DeviceMonitor::new(
            super::device_monitor::DeviceKind::Output,
            proxy,
        )
        .map_err(|e| format!("Failed to start device monitor: {e}"))?;

        Ok(Self {
            mixer,
            #[cfg(target_os = "macos")]
            _device_monitor: device_monitor,
        })
    }

    pub fn mixer(&self) -> &MixerHandle {
        &self.mixer
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
        for device in &sources {
            println!("  - {}", device);
        }

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

        let player = Player::new().expect("Failed to create player");
        let mixer = player.mixer();

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
