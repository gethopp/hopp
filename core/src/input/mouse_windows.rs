use std::{
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

use crate::{utils::geometry::Position, MouseClickData, ScrollDelta};

use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WAIT_TIMEOUT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
            MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
            MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetSystemMetrics, MsgWaitForMultipleObjects,
            PeekMessageW, SetCursorPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
            MSG, MSLLHOOKSTRUCT, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WH_MOUSE_LL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        },
    },
};

use super::{CursorSimulatorFunctions, SharerCursor, CUSTOM_MOUSE_EVENT};

// This is safe to do because the callback is not accessed after the hook is set up. It
// could fail only during destruction if a mouse event is received at the same time.
// This will be improved in the future, for now we accept the risk.
// The callback returns whether the event should be kept or not
type EventCallback = Box<dyn FnMut(i32, i32, u32, usize) -> bool>;
static mut EVENT_CALLBACK: Option<EventCallback> = None;

unsafe extern "system" fn mouse_hook(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let mouse_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
    let event_type = w_param.0 as u32;

    log::debug!("mouse_hook: n_code: {n_code}, event: {event_type}, l_param: {mouse_struct:?}");

    #[allow(static_mut_refs)]
    let keep = if let Some(callback) = EVENT_CALLBACK.as_mut() {
        let x = mouse_struct.pt.x;
        let y = mouse_struct.pt.y;
        callback(x, y, event_type, mouse_struct.dwExtraInfo)
    } else {
        true
    };

    if !keep {
        /* Non zero value block the event from propagating */
        return LRESULT(1);
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

enum EventProcessingCommand {
    SimulatedEvent(Position),
    Local(u32, Position),
    Stop,
}

fn event_processing_thread(
    sharer_cursor: Arc<Mutex<SharerCursor>>,
    receiver: std::sync::mpsc::Receiver<EventProcessingCommand>,
) {
    let mut last_location = Position::default();
    /*
     * On Windows we noticed that when the controller has control and
     * both the sharer and the controller are moving their cursors the
     * drawed sharer's cursor might experience jumps. For example,
     * let's consider the scenario where the sharer is moving horizontally
     * and the controller is doing circles.
     *    - sharer new location { x: 2426.0, y: 660.0 } old location { x: 2428.0, y: 660.0 }
     *    - sharer new location { x: 2420.0, y: 660.0 } old location { x: 2428.0, y: 660.0 }
     *    - controller new location { x: 2440.0, y: 650.0 }
     *    - controller new location { x: 2454.0, y: 641.0 }
     *    - sharer new location { x: 2440.0, y: 651.0 } old location { x: 2454.0, y: 641.0 }
     *
     * We can see in the above logs that when we got the last sharer event from the system
     * the origin before the move was the second to last controller location and not the last
     * (see the jump in y).
     *
     * For this reason we just ignore the first sharer event when we get a simulated one.
     * This is a simple solution that worked in our testing. We will refine it if needed.
     */
    let mut ignore_local_count = 0;
    loop {
        match receiver.recv() {
            Ok(command) => match command {
                EventProcessingCommand::SimulatedEvent(location) => {
                    last_location = location;
                    ignore_local_count = 1;
                }
                EventProcessingCommand::Local(event_type, location) => {
                    let mut sharer_cursor = sharer_cursor.lock().unwrap();
                    if event_type == WM_MOUSEMOVE {
                        if sharer_cursor.has_control() {
                            sharer_cursor.set_position(location);
                        } else {
                            if ignore_local_count > 0 {
                                ignore_local_count -= 1;
                                continue;
                            }

                            let dx = location.x - last_location.x;
                            let dy = location.y - last_location.y;

                            let global_position = sharer_cursor.global_position();
                            let sharer_left_monitor = sharer_cursor.set_position(Position {
                                x: global_position.x + dx,
                                y: global_position.y + dy,
                            });

                            if !sharer_left_monitor {
                                unsafe {
                                    let _ = SetCursorPos(
                                        last_location.x as i32,
                                        last_location.y as i32,
                                    );
                                }
                            }
                        }
                    } else if event_type == WM_MOUSEWHEEL {
                        sharer_cursor.scroll();
                    } else {
                        sharer_cursor.click();
                    }
                }
                EventProcessingCommand::Stop => {
                    break;
                }
            },
            Err(e) => {
                log::error!("event_processing_thread receive failed {e:?}");
                break;
            }
        }
    }
    log::info!("terminated event_processing_thread");
}

#[derive(Debug, thiserror::Error)]
pub enum MouseObserverError {
    #[error("Failed to set mouse hook")]
    SetMouseHook,
}

pub struct MouseObserver {
    hook_thread: Option<JoinHandle<()>>,
    tx_shutdown: Sender<EventProcessingCommand>,
}

impl MouseObserver {
    pub fn new(sharer_cursor: Arc<Mutex<SharerCursor>>) -> Result<Self, MouseObserverError> {
        /* Run the event callback in a separate thread. */
        let (hook_sender, hook_receiver) = std::sync::mpsc::channel();
        let (tx_shutdown, rx_shutdown) = std::sync::mpsc::channel();
        let hook_thread = std::thread::spawn(move || {
            let (sender, receiver) = std::sync::mpsc::channel();
            let sender_clone = sender.clone();
            let sharer_cursor_clone = sharer_cursor.clone();
            let callback = move |x: i32, y: i32, event_type: u32, extra_info: usize| {
                let location = Position {
                    x: x as f64,
                    y: y as f64,
                };

                if extra_info == (CUSTOM_MOUSE_EVENT as usize) {
                    sender_clone
                        .send(EventProcessingCommand::SimulatedEvent(location))
                        .unwrap();
                    true
                } else {
                    sender_clone
                        .send(EventProcessingCommand::Local(event_type, location))
                        .unwrap();
                    let sharer_cursor = sharer_cursor_clone.lock().unwrap();
                    sharer_cursor.has_control()
                }
            };

            let hook = unsafe {
                EVENT_CALLBACK = Some(Box::new(callback));

                let h_instance: HINSTANCE = GetModuleHandleW(None).unwrap().into();

                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), h_instance, 0);
                if hook.is_err()
                    && hook_sender
                        .send(Err(MouseObserverError::SetMouseHook))
                        .is_err()
                {
                    return;
                }

                hook.unwrap()
            };

            if let Err(e) = hook_sender.send(Ok(())) {
                log::error!("failed to send success message {e:?}");
                return;
            }

            let event_processing_thread =
                std::thread::spawn(move || event_processing_thread(sharer_cursor, receiver));
            unsafe {
                let mut msg = MSG::default();
                loop {
                    if rx_shutdown.try_recv().is_ok() {
                        break;
                    }

                    let result = MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
                    if result != WAIT_TIMEOUT {
                        while PeekMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0, PM_REMOVE)
                            .as_bool()
                        {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }

                let _ = UnhookWindowsHookEx(hook);
                EVENT_CALLBACK = None;
            }

            if sender.send(EventProcessingCommand::Stop).is_err() {
                log::error!("Failed to send stop command to event processing thread.");
            }
            let _ = event_processing_thread.join();
        });

        match hook_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                hook_thread: Some(hook_thread),
                tx_shutdown,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(MouseObserverError::SetMouseHook),
        }
    }
}

impl Drop for MouseObserver {
    fn drop(&mut self) {
        if let Some(handle) = self.hook_thread.take() {
            if self.tx_shutdown.send(EventProcessingCommand::Stop).is_err() {
                log::error!("Faled to send stop event to hook thread");
            }
            let _ = handle.join();
        }
        log::info!("terminated mouse observer");
    }
}

enum SendInputMessage {
    Input(INPUT),
    Stop,
}

fn send_input(input: &[INPUT]) {
    let input_size = std::mem::size_of::<INPUT>() as i32;
    let input_len = input.len() as u32;
    let result = unsafe { SendInput(input, input_size) };
    if result != input_len {
        log::error!("send_input: SendInput failed");
    }
}

fn send_input_thread(rx: mpsc::Receiver<SendInputMessage>) {
    loop {
        let msg = rx.recv();
        match msg {
            Ok(SendInputMessage::Stop) => {
                break;
            }
            Ok(SendInputMessage::Input(input)) => {
                send_input(&[input]);
            }
            _ => {
                log::error!("send_input_thread: Error receiving message");
                break;
            }
        }
    }
}

pub struct CursorSimulator {
    last_wheel_event: std::time::Instant,
    skipped_wheel_events: u32,
    tx: mpsc::Sender<SendInputMessage>,
    send_input_handle: Option<std::thread::JoinHandle<()>>,
}

impl Default for CursorSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorSimulator {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let send_input_handle = Some(std::thread::spawn(move || {
            send_input_thread(rx);
        }));

        Self {
            last_wheel_event: std::time::Instant::now(),
            skipped_wheel_events: 0,
            tx,
            send_input_handle,
        }
    }
}

impl Drop for CursorSimulator {
    fn drop(&mut self) {
        let _ = self.tx.send(SendInputMessage::Stop);
        if let Some(handle) = self.send_input_handle.take() {
            let _ = handle.join();
        }
    }
}

/*
 * We don't handle the x buttons so data should be 0 for now.
 */
fn mouse_event(flags: MOUSE_EVENT_FLAGS, data: i32, x: i32, y: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x,
                dy: y,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: CUSTOM_MOUSE_EVENT as usize,
            },
        },
    }
}

fn coords_to_virtual(x: f32, y: f32) -> (i32, i32) {
    let virtual_screen_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let virtual_screen_top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let virtual_screen_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let virtual_screen_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    log::debug!(
        "coords_to_virtual: virtual_screen_left: {virtual_screen_left}, virtual_screen_top: {virtual_screen_top}, virtual_screen_width: {virtual_screen_width}, virtual_screen_height: {virtual_screen_height}"
    );

    let x = (x - virtual_screen_left as f32) / virtual_screen_width as f32 * 65535.0;
    let y = (y - virtual_screen_top as f32) / virtual_screen_height as f32 * 65535.0;
    (x as i32, y as i32)
}

fn wheel_translation(value: f64) -> i32 {
    let sign = if value >= 0.0 { 1 } else { -1 };
    let abs = value.abs();
    let ret = if abs <= 40. {
        if abs < 10. && abs > 0. {
            120
        } else {
            (120. * (abs / 15.)) as i32
        }
    } else if abs <= 100. {
        (120. * (abs / 20.)) as i32
    } else if abs <= 200. {
        (abs * 2.) as i32
    } else {
        abs as i32
    };

    ret * sign
}

impl CursorSimulatorFunctions for CursorSimulator {
    fn simulate_cursor_movement(&mut self, position: Position, _click_down: bool) {
        log::debug!("simulate_cursor_movement: {position:?}");
        let (x, y) = coords_to_virtual(position.x as f32, position.y as f32);
        let mouse_event = mouse_event(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            0,
            x,
            y,
        );
        let res = self.tx.send(SendInputMessage::Input(mouse_event));
        if res.is_err() {
            log::error!("simulate_cursor_movement: Error sending message");
        }
    }

    fn simulate_click(&mut self, click_data: MouseClickData) {
        log::debug!("simulate_click: click_data: {click_data:?}",);

        let (x, y) = coords_to_virtual(click_data.x, click_data.y);
        log::debug!("simulate_click: converted coords x: {x}, y: {y}");

        let mouse_flag = if click_data.button == 0 {
            if click_data.down {
                MOUSEEVENTF_LEFTDOWN
            } else {
                MOUSEEVENTF_LEFTUP
            }
        } else if click_data.button == 2 {
            if click_data.down {
                MOUSEEVENTF_RIGHTDOWN
            } else {
                MOUSEEVENTF_RIGHTUP
            }
        } else if click_data.down {
            MOUSEEVENTF_MIDDLEDOWN
        } else {
            MOUSEEVENTF_MIDDLEUP
        };

        let mouse_event = mouse_event(
            mouse_flag | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            0,
            x,
            y,
        );
        let res = self.tx.send(SendInputMessage::Input(mouse_event));
        if res.is_err() {
            log::error!("simulate_click: Error sending message");
        }
    }

    /*
     * The lowest value we can send to SendInput for scrolling is 120
     * (equal to WHEEL_DELTA), which creates a compatibility issue with the
     * delta values we get from the browser, which are in pixels.
     *
     * Also the values we get from a touchpad are different from the values we
     * get from a mouse. A touchpad has smaller deltas but is generating more
     * events compared to a mouse.
     *
     * We try to work around this by using a different multiplying factor to
     * 120 for different input values, below 100 the factor is bigger and as
     * delta is increasing the factor become smaller and eventually 1. We want
     *
     * Another issue is that on windows it seems that the events have lower
     * sampling from macos, which could end up in endless scrolling if the
     * controller is using macos and the sharer windows.
     *
     * For this reason we check the duration between two consecutive events
     * and if there are too close to each other (less than 15ms), we reduce the
     * sampling.
     */
    fn simulate_scroll(&mut self, delta: ScrollDelta) {
        let elapsed = self.last_wheel_event.elapsed().as_millis();
        self.last_wheel_event = std::time::Instant::now();
        log::debug!("simulate_scroll: delta: {delta:?} elapsed: {elapsed:?}ms");

        if elapsed < 15 && self.skipped_wheel_events < 50 {
            self.skipped_wheel_events += 1;
            return;
        }
        log::debug!(
            "simulate_scroll: self.skipped_wheel_events {}",
            self.skipped_wheel_events
        );

        self.skipped_wheel_events = 0;

        let data_x = wheel_translation(delta.x);
        let data_y = wheel_translation(delta.y);
        let horizontal_event = mouse_event(MOUSEEVENTF_HWHEEL, data_x, 0, 0);
        let vertical_event = mouse_event(MOUSEEVENTF_WHEEL, data_y, 0, 0);

        let res = self.tx.send(SendInputMessage::Input(horizontal_event));
        if res.is_err() {
            log::error!("simulate_scroll: Error sending message");
        }
        let res = self.tx.send(SendInputMessage::Input(vertical_event));
        if res.is_err() {
            log::error!("simulate_scroll: Error sending message");
        }
    }
}
