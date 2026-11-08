//! Observations and native action entry points compiled only for integration testing.
use std::sync::atomic::{AtomicU64, Ordering};

use socket_lib::{TestAction, TestCallControl, TestStatus};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use crate::components::call_controls::{CallControlsMessage, CallControlsState};
use crate::{Application, UserEvent};

pub(crate) static SCREEN_CAPTURED: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCREEN_RECEIVED: AtomicU64 = AtomicU64::new(0);
pub(crate) static CAMERA_RECEIVED: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCREEN_PRESENTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static CAMERA_PRESENTED: AtomicU64 = AtomicU64::new(0);
static AUDIO_RECEIVED: AtomicU64 = AtomicU64::new(0);
static AUDIO_RECEIVED_NONZERO: AtomicU64 = AtomicU64::new(0);
static AUDIO_MIXED: AtomicU64 = AtomicU64::new(0);
static AUDIO_MIXED_NONZERO: AtomicU64 = AtomicU64::new(0);
static AUDIO_OUTPUT: AtomicU64 = AtomicU64::new(0);
static AUDIO_OUTPUT_NONZERO: AtomicU64 = AtomicU64::new(0);

pub(crate) fn record_audio_received(samples: &[i16]) {
    AUDIO_RECEIVED.fetch_add(1, Ordering::Relaxed);
    AUDIO_RECEIVED_NONZERO.fetch_add(nonzero_samples(samples), Ordering::Relaxed);
}

pub(crate) fn record_audio_mixed(samples: &[i16]) {
    AUDIO_MIXED.fetch_add(1, Ordering::Relaxed);
    AUDIO_MIXED_NONZERO.fetch_add(nonzero_samples(samples), Ordering::Relaxed);
}

pub(crate) fn record_audio_output(samples: &[f32]) {
    AUDIO_OUTPUT.fetch_add(samples.len() as u64, Ordering::Relaxed);
    AUDIO_OUTPUT_NONZERO.fetch_add(
        samples
            .iter()
            .filter(|sample| sample.abs() > 1.0 / 32768.0)
            .count() as u64,
        Ordering::Relaxed,
    );
}

fn nonzero_samples(samples: &[i16]) -> u64 {
    samples
        .iter()
        .filter(|sample| sample.unsigned_abs() > 1)
        .count() as u64
}

pub(crate) fn control_message(
    control: TestCallControl,
    state: &CallControlsState,
) -> CallControlsMessage {
    state.test_message(control)
}

impl Application<'_> {
    pub(crate) fn test_status(&self) -> TestStatus {
        let local = self.room_service.as_ref().and_then(|room| {
            let participants = room.participants();
            let guard = participants.read().ok()?;
            let local = guard.get("local")?;
            Some((local.is_screensharing(), local.muted()))
        });
        TestStatus {
            call_id: self.current_call_id.clone(),
            camera_visible: self
                .camera_window
                .as_ref()
                .is_some_and(|window| window.is_visible()),
            camera_active: self
                .camera_capturer
                .lock()
                .unwrap()
                .active_device_name()
                .is_some(),
            viewer_visible: self
                .screensharing_window
                .as_ref()
                .is_some_and(|window| window.is_visible()),
            drawing_visible: self
                .drawing_window
                .as_ref()
                .is_some_and(|window| window.is_visible()),
            stats_visible: self
                .stats_window
                .as_ref()
                .is_some_and(|window| window.is_visible()),
            picker_open: self.screen_selection.is_some(),
            mic_capturing: self.audio_capturer.is_capturing(),
            mic_muted: local.is_none_or(|(_, muted)| muted),
            local_sharing: local.is_some_and(|(sharing, _)| sharing),
            participant_in_control: self
                .remote_control
                .as_ref()
                .and_then(|remote| remote.cursor_controller.test_participant_in_control()),
            local_in_control: self
                .screensharing_window
                .as_ref()
                .is_some_and(|window| window.local_in_control()),
            screen_frames_captured: SCREEN_CAPTURED.load(Ordering::Relaxed),
            screen_frames_received: SCREEN_RECEIVED.load(Ordering::Relaxed),
            camera_frames_received: CAMERA_RECEIVED.load(Ordering::Relaxed),
            screen_frames_presented: SCREEN_PRESENTED.load(Ordering::Relaxed),
            camera_frames_presented: CAMERA_PRESENTED.load(Ordering::Relaxed),
            audio_frames_received: AUDIO_RECEIVED.load(Ordering::Relaxed),
            audio_frames_mixed: AUDIO_MIXED.load(Ordering::Relaxed),
            audio_output_samples: AUDIO_OUTPUT.load(Ordering::Relaxed),
            audio_received_nonzero_samples: AUDIO_RECEIVED_NONZERO.load(Ordering::Relaxed),
            audio_mixed_nonzero_samples: AUDIO_MIXED_NONZERO.load(Ordering::Relaxed),
            audio_output_nonzero_samples: AUDIO_OUTPUT_NONZERO.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn test_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action: TestAction,
    ) -> Result<(), String> {
        match action {
            TestAction::CloseCameraWindow => {
                let window = self.camera_window.as_ref().ok_or("camera window absent")?;
                self.window_event(event_loop, window.window_id(), WindowEvent::CloseRequested);
            }
            TestAction::CloseViewerWindow => {
                let window = self
                    .screensharing_window
                    .as_ref()
                    .ok_or("viewer window absent")?;
                self.window_event(event_loop, window.window_id(), WindowEvent::CloseRequested);
            }
            TestAction::CloseStatsWindow => {
                let window = self.stats_window.as_ref().ok_or("stats window absent")?;
                self.window_event(event_loop, window.window_id(), WindowEvent::CloseRequested);
            }
            TestAction::CancelPicker => self.cancel_screen_selection(),
            TestAction::RemoteControlEnabled(enabled) => {
                self.user_event(event_loop, UserEvent::ControllerCursorEnabled(enabled));
            }
            TestAction::CameraControl(control) => {
                self.camera_window
                    .as_mut()
                    .ok_or("camera window absent")?
                    .test_control(control);
            }
            TestAction::ViewerControl(control) => {
                self.screensharing_window
                    .as_mut()
                    .ok_or("viewer window absent")?
                    .test_control(control);
            }
        }
        Ok(())
    }
}

use winit::application::ApplicationHandler;

#[cfg(test)]
mod tests {
    #[test]
    fn meaningful_audio_ignores_silence_and_quantization_noise() {
        assert_eq!(super::nonzero_samples(&[0, 1, -1, 2, -2, i16::MIN]), 3);
    }
}
