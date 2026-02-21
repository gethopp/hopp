use rodio::source::Zero;
use rodio::{DeviceSinkBuilder, MixerDeviceSink};
use std::num::NonZero;

use super::rodio_mixer::RodioMixerHandle;

pub struct RodioPlayer {
    _sink: MixerDeviceSink,
    mixer: RodioMixerHandle,
}

impl RodioPlayer {
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
            mixer: RodioMixerHandle::new(rodio_mixer),
        })
    }

    pub fn mixer(&self) -> &RodioMixerHandle {
        &self.mixer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::rodio_capturer::RodioCapturer;
    use crate::audio::rodio_mixer::RodioAudioSource;
    use std::time::Duration;

    #[test]
    fn test_dual_capture_and_playback() {
        let mut capturer1 = RodioCapturer::new();
        let mut capturer2 = RodioCapturer::new();

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

        let player = RodioPlayer::new().expect("Failed to create player");
        let mixer = player.mixer();

        let source1 = RodioAudioSource::new(16000, 1);
        let source2 = RodioAudioSource::new(16000, 1);

        let s1 = source1.clone();
        let s2 = source2.clone();

        mixer.add_source(&source1);
        mixer.add_source(&source2);

        capturer1.start_capture(None, tx1).expect("capture 1");
        capturer2.start_capture(None, tx2).expect("capture 2");

        // Forward captured samples → mixer (simulates LiveKit push)
        std::thread::spawn(move || {
            while let Some(samples) = rx1.blocking_recv() {
                s1.push_samples(samples);
            }
        });
        std::thread::spawn(move || {
            while let Some(samples) = rx2.blocking_recv() {
                s2.push_samples(samples);
            }
        });

        println!("Playing two simultaneous captures for 5 seconds...");
        std::thread::sleep(Duration::from_secs(5));

        capturer1.stop_capture();
        capturer2.stop_capture();
        std::thread::sleep(Duration::from_millis(500));
    }
}
