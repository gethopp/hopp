use std::sync::{Arc, Mutex};

use crate::{
    input::mouse::SharerCursor, overlay_window::OverlayWindow, utils::geometry::Position,
    MouseClickData, ScrollDelta,
};

use core_foundation::{
    base::TCFType,
    mach_port::CFMachPortInvalidate,
    runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop},
};
use core_graphics::{
    display::{CGPoint, CGWarpMouseCursorPosition},
    event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, CallbackResult,
        EventField, ScrollEventUnit,
    },
};
use core_graphics::{
    event::{CGEventTap, CGEventTapOptions, CGEventTapPlacement},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_graphics::{CGEvent as ObjcCGEvent, CGEventType as ObjcCGEventType};
use objc2_foundation::{NSPoint, NSProcessInfo};

use super::{CursorSimulatorFunctions, CUSTOM_MOUSE_EVENT};

const EVENT_TAP_DURATION_MS: u64 = 250;

#[derive(Debug, thiserror::Error)]
pub enum MouseObserverError {
    #[error("Failed to create mouse tap")]
    CreateMouseTap,
    #[error("Failed to create runloop source")]
    CreateRunloopSource,
}

/// Owns the macOS event-tap thread that calls into `SharerCursor` from CGEventTap
/// callbacks. A clone of the `Arc<Mutex<SharerCursor>>` is moved into the tap closure
/// (see `MouseObserver::new`).
///
/// LOCK ORDER (enforced from the tap thread): `sharer_cursor` is locked BEFORE
/// `controllers_cursors`. Any other thread that needs both MUST follow the same order
/// — i.e. release `CursorController::controllers_cursors` before locking
/// `sharer_cursor`. Violating this order deadlocks (ABBA) under simultaneous local
/// (hardware) + remote (room-event) scroll or click.
pub struct MouseObserver {
    shutdown_tx: std::sync::mpsc::Sender<()>,
}

enum MouseTapCreationResult {
    Success,
    Error(MouseObserverError),
}

impl MouseObserver {
    pub fn new(internal: Arc<Mutex<SharerCursor>>) -> Result<Self, MouseObserverError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

        /* We run the event tap in separate thread to avoid blocking and getting blocked by the main thread. */
        std::thread::spawn(move || {
            let tap_disabled = Arc::new(Mutex::new(false));
            let tap_disabled_clone = tap_disabled.clone();
            let mouse_tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![
                    CGEventType::LeftMouseDown,
                    CGEventType::RightMouseDown,
                    CGEventType::MouseMoved,
                    CGEventType::LeftMouseDragged,
                    CGEventType::ScrollWheel,
                ],
                move |_a, _b, d| {
                    /* Ignore the event when is our own click. */
                    let user_data = d.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA);
                    log::debug!(
                        "Mouse callback event received type {:?} location {:?} user_data {}",
                        d.get_type(),
                        d.location(),
                        user_data
                    );

                    if user_data == CUSTOM_MOUSE_EVENT {
                        return CallbackResult::Keep;
                    }

                    match d.get_type() {
                        CGEventType::MouseMoved | CGEventType::LeftMouseDragged => {
                            log::debug!("Mouse moved event received");

                            let mut sharer_cursor = internal.lock().unwrap();
                            let sharer_has_control = sharer_cursor.has_control();

                            let location = Position {
                                x: d.location().x,
                                y: d.location().y,
                            };
                            let last_event_position = sharer_cursor.get_last_event_position();
                            sharer_cursor.set_last_event_position(location);

                            if sharer_has_control {
                                sharer_cursor.set_position(Position {
                                    x: location.x,
                                    y: location.y,
                                });
                            } else {
                                let sharer_position = sharer_cursor.global_position();
                                log::debug!("sharer_position: {sharer_position:?}");

                                let mut dx =
                                    d.get_double_value_field(EventField::MOUSE_EVENT_DELTA_X);
                                let mut dy =
                                    d.get_double_value_field(EventField::MOUSE_EVENT_DELTA_Y);
                                log::debug!("dx: {dx}, dy: {dy}");

                                let dx_delta = location.x - last_event_position.x;
                                let dy_delta = location.y - last_event_position.y;
                                log::debug!("dx_delta: {dx_delta}, dy_delta: {dy_delta}");

                                /*
                                 * Because macOS doesn't register the delta of simulated
                                 * events, we need to subtract the delta of the last hardware
                                 * event.
                                 */
                                dx -= dx_delta;
                                dy -= dy_delta;

                                let sharer_left_monitor = sharer_cursor.set_position(Position {
                                    x: sharer_position.x + dx,
                                    y: sharer_position.y + dy,
                                });

                                if !sharer_left_monitor {
                                    unsafe {
                                        CGWarpMouseCursorPosition(CGPoint {
                                            x: location.x,
                                            y: location.y,
                                        });
                                    }
                                }

                                return CallbackResult::Drop;
                            }
                        }
                        CGEventType::ScrollWheel => {
                            log::debug!("Scroll wheel event received");
                            let mut sharer_cursor = internal.lock().unwrap();
                            let sharer_has_control = sharer_cursor.has_control();
                            sharer_cursor.scroll();
                            if !sharer_has_control {
                                let sharer_position = sharer_cursor.global_position();
                                unsafe {
                                    CGWarpMouseCursorPosition(CGPoint::new(
                                        sharer_position.x,
                                        sharer_position.y,
                                    ));
                                }
                            }
                        }
                        CGEventType::TapDisabledByTimeout => {
                            log::error!("Tap disabled by timeout");
                            sentry_utils::upload_logs_event("Tap disabled by timeout".to_string());
                            *tap_disabled_clone.lock().unwrap() = true;
                        }
                        _ => {
                            let mut sharer_cursor = internal.lock().unwrap();
                            let sharer_has_control = sharer_cursor.has_control();

                            // On click transition we just overwrite the event's location
                            if !sharer_has_control {
                                let sharer_position = sharer_cursor.global_position();
                                sharer_cursor.hide(true);
                                unsafe {
                                    CGWarpMouseCursorPosition(CGPoint::new(
                                        sharer_position.x,
                                        sharer_position.y,
                                    ));
                                }
                                d.set_location(CGPoint::new(sharer_position.x, sharer_position.y));
                            }
                        }
                    }

                    CallbackResult::Keep
                },
            );
            let mouse_tap = match mouse_tap {
                Ok(mouse_tap) => mouse_tap,
                Err(()) => {
                    let _ = tx.send(MouseTapCreationResult::Error(
                        MouseObserverError::CreateMouseTap,
                    ));
                    return;
                }
            };

            let current_loop = CFRunLoop::get_current();
            let loop_source = unsafe {
                let loop_source = match mouse_tap.mach_port().create_runloop_source(0) {
                    Ok(loop_source) => loop_source,
                    Err(()) => {
                        let _ = tx.send(MouseTapCreationResult::Error(
                            MouseObserverError::CreateRunloopSource,
                        ));
                        return;
                    }
                };
                current_loop.add_source(&loop_source, kCFRunLoopCommonModes);
                mouse_tap.enable();
                loop_source
            };
            let _ = tx.send(MouseTapCreationResult::Success);

            loop {
                if shutdown_rx.try_recv().is_ok() {
                    log::debug!("MouseObserver::new: shutdown requested");
                    break;
                }
                unsafe {
                    CFRunLoop::run_in_mode(
                        kCFRunLoopDefaultMode,
                        std::time::Duration::from_millis(EVENT_TAP_DURATION_MS),
                        false,
                    );
                }
                let mut tap_disabled = tap_disabled.lock().unwrap();
                if *tap_disabled {
                    log::info!("MouseObserver::new: re enable tap");
                    mouse_tap.enable();
                    *tap_disabled = false;
                }
            }

            unsafe {
                current_loop.remove_source(&loop_source, kCFRunLoopCommonModes);
                CFMachPortInvalidate(mouse_tap.mach_port().as_CFTypeRef() as *mut _);
            }
        });

        match rx.recv() {
            Ok(result) => match result {
                MouseTapCreationResult::Success => {}
                MouseTapCreationResult::Error(error) => {
                    log::error!(
                        "MouseObserver::new: error receiving mouse tap creation result: {error:?}"
                    );
                    return Err(error);
                }
            },
            Err(e) => {
                log::error!("MouseObserver::new: error receiving mouse tap creation result: {e:?}");
                return Err(MouseObserverError::CreateMouseTap);
            }
        };

        Ok(Self { shutdown_tx })
    }
}

impl Drop for MouseObserver {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}

pub struct CursorSimulator {
    overlay_window: Arc<OverlayWindow>,
    target_process_id: Option<i32>,
    target_window_id: Option<u32>,
    last_position: Option<Position>,
    /// Cached result of `measure_sender_flip_height`. Probing posts synthetic
    /// NSEvents through AppKit on the event hot path, so it only runs once and
    /// is invalidated via `invalidate_sender_flip_height` when the capture
    /// frame changes (window moved/resized/changed monitors).
    sender_flip_height: Option<f64>,
}

impl Default for CursorSimulator {
    fn default() -> Self {
        Self::new(Arc::new(OverlayWindow::default()), None, None)
    }
}

impl CursorSimulator {
    pub fn new(
        overlay_window: Arc<OverlayWindow>,
        target_process_id: Option<i32>,
        target_window_id: Option<u32>,
    ) -> Self {
        Self {
            overlay_window,
            target_process_id,
            target_window_id,
            last_position: None,
            sender_flip_height: None,
        }
    }

    pub fn invalidate_sender_flip_height(&mut self) {
        self.sender_flip_height = None;
    }

    /// Whether events are being delivered directly to a shared window's
    /// process (window sharing with pinned delivery) rather than system-wide.
    pub fn has_window_target(&self) -> bool {
        self.target_process_id.is_some()
    }

    /// Measures the height the NSEvent->CGEvent conversion uses to flip AppKit
    /// screen coordinates into global ones (the main display's height) by
    /// posting probe events through the same conversion path used for delivery.
    ///
    /// Returns `None` when the probe fails or the measured transform has an
    /// unexpected shape, in which case the caller falls back to the heuristic
    /// in `OverlayWindow::global_to_window_local`.
    fn measure_sender_flip_height(&self, window_id: u32) -> Option<f64> {
        let probe = |x: f64, y: f64| -> Option<(f64, f64)> {
            let timestamp = NSProcessInfo::processInfo().systemUptime();
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                NSEventType(CGEventType::MouseMoved as usize),
                NSPoint::new(x, y),
                NSEventModifierFlags(0),
                timestamp,
                window_id as isize,
                None,
                0,
                0,
                0.0,
            )?;
            let cg_event = event.CGEvent()?;
            let location = ObjcCGEvent::location(Some(&cg_event));
            Some((location.x, location.y))
        };
        let (origin_x, origin_y) = probe(0.0, 0.0)?;
        let (unit_x, unit_y) = probe(1.0, 1.0)?;
        // Expected transform: (x, y) -> (x, height - y).
        if (unit_x - origin_x - 1.0).abs() > 0.001 || (unit_y - origin_y + 1.0).abs() > 0.001 {
            log::error!(
                "measure_sender_flip_height: unexpected transform ({origin_x}, {origin_y}) -> ({unit_x}, {unit_y})"
            );
            return None;
        }
        Some(origin_y)
    }

    fn post_to_window(
        &mut self,
        event_type: CGEventType,
        position: Position,
        flags: CGEventFlags,
        click_count: i64,
        posted_event_type: Option<CGEventType>,
    ) -> bool {
        let (Some(process_id), Some(window_id)) = (self.target_process_id, self.target_window_id)
        else {
            return false;
        };
        let flip_height = match self.sender_flip_height {
            Some(height) => Some(height),
            None => {
                let measured = self.measure_sender_flip_height(window_id);
                if measured.is_some() {
                    self.sender_flip_height = measured;
                }
                measured
            }
        };
        let Some(location) = self
            .overlay_window
            .global_to_window_local(position, flip_height)
        else {
            log::error!("post_to_window: invalid capture frame or mouse position");
            return true;
        };
        let timestamp = NSProcessInfo::processInfo().systemUptime();
        let Some(ns_event) =
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                NSEventType(event_type as usize),
                NSPoint::new(location.x, location.y),
                NSEventModifierFlags(flags.bits() as usize),
                timestamp,
                window_id as isize,
                None,
                0,
                click_count as isize,
                0.,
            )
        else {
            log::error!("post_to_window: failed to create NSEvent");
            return true;
        };
        let Some(cg_event) = ns_event.CGEvent() else {
            log::error!("post_to_window: failed to convert NSEvent to CGEvent");
            return true;
        };
        if let Some(posted_event_type) = posted_event_type {
            ObjcCGEvent::set_type(Some(&cg_event), ObjcCGEventType(posted_event_type as u32));
        }
        // AppKit may consume the first mouse-down to activate an unfocused
        // window, so click delivery depends on the target view's first-mouse policy.
        // This means that when a window is unfocused a click might not go. The alternative
        // is to use private APIs to handle this properly.
        ObjcCGEvent::post_to_pid(process_id, Some(&cg_event));
        true
    }

    fn post(&self, event: &CGEvent) {
        if let Some(process_id) = self.target_process_id {
            event.post_to_pid(process_id);
        } else {
            event.post(CGEventTapLocation::HID);
        }
    }
}

impl CursorSimulatorFunctions for CursorSimulator {
    fn simulate_cursor_movement(&mut self, position: Position, click_down: bool) {
        log::debug!("simulate_cursor_movement: {position:?}");
        self.last_position = Some(position);
        let event_type = if click_down {
            CGEventType::LeftMouseDragged
        } else {
            CGEventType::MouseMoved
        };
        if self.post_to_window(event_type, position, CGEventFlags::empty(), 0, None) {
            /*
             * Pinned delivery doesn't move the system cursor. Warp it without
             * posting an event, so the cursor follows without the move also
             * being delivered to the shared window a second time.
             */
            unsafe {
                CGWarpMouseCursorPosition(CGPoint::new(position.x, position.y));
            }
            return;
        }
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(error) => {
                log::error!("simulate_cursor_movement: error creating event source: {error:?}");
                return;
            }
        };
        let event = CGEvent::new_mouse_event(
            event_source,
            event_type,
            CGPoint::new(position.x, position.y),
            CGMouseButton::Center,
        );
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                log::error!("simulate_cursor_movement: error creating mouse event: {error:?}");
                return;
            }
        };

        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, CUSTOM_MOUSE_EVENT);
        event.post(CGEventTapLocation::HID);
    }

    fn simulate_click(&mut self, click_data: MouseClickData) {
        log::debug!("simulate_click: click_data: {click_data:?}",);
        let mut event_flags = CGEventFlags::empty();
        if click_data.shift {
            event_flags.insert(CGEventFlags::CGEventFlagShift);
        }
        if click_data.ctrl {
            event_flags.insert(CGEventFlags::CGEventFlagControl);
        }
        if click_data.alt {
            event_flags.insert(CGEventFlags::CGEventFlagAlternate);
        }
        if click_data.meta {
            event_flags.insert(CGEventFlags::CGEventFlagCommand);
        }

        /* The button value is interpreted based on https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/button  */
        // TODO: Handle other mouse button values
        let (mouse_dir, mouse_button) = if click_data.button == 0 {
            (
                if click_data.down {
                    CGEventType::LeftMouseDown
                } else {
                    CGEventType::LeftMouseUp
                },
                CGMouseButton::Left,
            )
        } else if click_data.button == 2 {
            (
                if click_data.down {
                    CGEventType::RightMouseDown
                } else {
                    CGEventType::RightMouseUp
                },
                CGMouseButton::Right,
            )
        } else {
            (
                if click_data.down {
                    CGEventType::OtherMouseDown
                } else {
                    CGEventType::OtherMouseUp
                },
                CGMouseButton::Left,
            )
        };
        log::debug!("simulate_click: mouse_dir: {mouse_dir:?} mouse_button: {mouse_button:?}");

        if self.post_to_window(
            mouse_dir,
            Position {
                x: click_data.x as f64,
                y: click_data.y as f64,
            },
            event_flags,
            click_data.clicks as i64,
            None,
        ) {
            return;
        }
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(error) => {
                log::error!("simulate_click: error creating event source: {error:?}");
                return;
            }
        };
        let event = CGEvent::new_mouse_event(
            event_source.clone(),
            mouse_dir,
            CGPoint::new(click_data.x as f64, click_data.y as f64),
            mouse_button,
        );
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                log::error!("simulate_click: error creating mouse event: {error:?}");
                return;
            }
        };
        event.set_integer_value_field(
            EventField::MOUSE_EVENT_CLICK_STATE,
            click_data.clicks as i64,
        );
        event.set_flags(event_flags);
        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, CUSTOM_MOUSE_EVENT);
        self.post(&event);
    }

    fn simulate_scroll(&mut self, delta: ScrollDelta) {
        log::debug!("simulate_scroll: delta: {delta:?}",);

        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(error) => {
                log::error!("simulate_scroll: error creating event source: {error:?}");
                return;
            }
        };
        let event = CGEvent::new_scroll_event(
            event_source,
            ScrollEventUnit::PIXEL,
            2,
            delta.y as i32,
            delta.x as i32,
            0,
        );
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                log::error!("simulate_scroll: error creating scroll event: {error:?}");
                return;
            }
        };
        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, CUSTOM_MOUSE_EVENT);
        if self.target_process_id.is_some() {
            let Some(position) = self.last_position else {
                log::error!("simulate_scroll: target window position is unavailable");
                return;
            };
            // A bare wheel event posted to a PID has no target-window location. Prime
            // AppKit with an NSEvent-derived scroll event before posting the deltas.
            self.post_to_window(
                CGEventType::MouseMoved,
                position,
                CGEventFlags::empty(),
                0,
                Some(CGEventType::ScrollWheel),
            );
        }
        self.post(&event);
    }
}
