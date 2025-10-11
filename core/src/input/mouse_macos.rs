use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use crate::{input::mouse::SharerCursor, utils::geometry::Position, MouseClickData, ScrollDelta};

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

use super::{CursorSimulatorFunctions, CUSTOM_MOUSE_EVENT};

const EVENT_TAP_DURATION_MS: u64 = 250;

#[derive(Debug, thiserror::Error)]
pub enum MouseObserverError {
    #[error("Failed to create mouse tap")]
    CreateMouseTap,
    #[error("Failed to create runloop source")]
    CreateRunloopSource,
}

pub struct MouseObserver {
    event_tap_thread: Option<JoinHandle<()>>,
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
        let event_tap_thread = std::thread::spawn(move || {
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
                        CGEventType::MouseMoved => {
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

                                sharer_cursor.set_position(Position {
                                    x: sharer_position.x + dx,
                                    y: sharer_position.y + dy,
                                });

                                unsafe {
                                    CGWarpMouseCursorPosition(CGPoint {
                                        x: location.x,
                                        y: location.y,
                                    });
                                }

                                return CallbackResult::Drop;
                            }
                        }
                        CGEventType::ScrollWheel => {
                            log::debug!("Scroll wheel event received");
                            let mut sharer_cursor = internal.lock().unwrap();
                            sharer_cursor.scroll();
                        }
                        CGEventType::TapDisabledByTimeout => {
                            log::error!("Tap disabled by timeout");
                            sentry_utils::upload_logs_event("Tap disabled by timeout".to_string());
                            *tap_disabled_clone.lock().unwrap() = true;
                        }
                        _ => {
                            log::debug!("Any other event received");
                            let mut sharer_cursor = internal.lock().unwrap();
                            let sharer_has_control = sharer_cursor.has_control();
                            sharer_cursor.click();
                            /*
                             * We drop the first one where the sharer doesn't have control
                             * and we simulate it the click down. Inside mouse_click_sharer.
                             */
                            if !sharer_has_control {
                                /* We need to put the cursor where the sharer is. */
                                return CallbackResult::Drop;
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

        Ok(Self {
            event_tap_thread: Some(event_tap_thread),
            shutdown_tx,
        })
    }
}

impl Drop for MouseObserver {
    fn drop(&mut self) {
        if let Some(event_tap_thread) = self.event_tap_thread.take() {
            let _ = self.shutdown_tx.send(());
            let _ = event_tap_thread.join();
        }
    }
}

pub struct CursorSimulator {}

impl Default for CursorSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorSimulator {
    pub fn new() -> Self {
        Self {}
    }
}

impl CursorSimulatorFunctions for CursorSimulator {
    fn simulate_cursor_movement(&mut self, position: Position, click_down: bool) {
        log::debug!("simulate_cursor_movement: {position:?}");
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(error) => {
                log::error!("simulate_cursor_movement: error creating event source: {error:?}");
                return;
            }
        };
        let event_type = if click_down {
            CGEventType::LeftMouseDragged
        } else {
            CGEventType::MouseMoved
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
        event.post(CGEventTapLocation::HID);
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
        event.post(CGEventTapLocation::HID);
    }
}
