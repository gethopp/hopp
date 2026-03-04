use rodio::source::Zero;
use rodio::{DeviceSinkBuilder, MixerDeviceSink};
use std::num::NonZero;

use super::mixer::MixerHandle;

pub struct Player {
    _sink: MixerDeviceSink,
    mixer: MixerHandle,
}

impl Player {
    pub fn new() -> Result<Self, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open default sink: {e}"))?;
        sink.log_on_drop(false);

        let rodio_mixer = sink.mixer().clone();

        // Infinite silence keeps mixer attached to the output stream.
        rodio_mixer.add(Zero::new(
            NonZero::new(1u16).unwrap(),
            NonZero::new(16000u32).unwrap(),
        ));

        Ok(Self {
            _sink: sink,
            mixer: MixerHandle::new(rodio_mixer),
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
        for (name, is_default) in &sources {
            println!("  - {} (default: {})", name, is_default);
        }

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

        let player = Player::new().expect("Failed to create player");
        let mixer = player.mixer();

        let s1 = mixer.add_source(16000, 1);
        let s2 = mixer.add_source(16000, 1);

        capturer1.start_capture(None, tx1).expect("capture 1");
        capturer2.start_capture(None, tx2).expect("capture 2");

        // Forward captured samples → mixer (simulates LiveKit push)
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
