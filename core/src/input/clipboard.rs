use crate::room_service;
use crate::KeystrokeData;
use crate::{KeyboardController, KeyboardLayout};
use thiserror::Error;

pub struct ClipboardController {
    clipboard: arboard::Clipboard,
    clipboard_payload: Vec<room_service::ClipboardPayload>,
}

fn construct_clipboard_data(clipboard_payload: &mut [room_service::ClipboardPayload]) -> String {
    clipboard_payload.sort_by_key(|p| p.packet_id);

    let combined_data: Vec<u8> = clipboard_payload
        .iter_mut()
        .flat_map(|payload| payload.data.clone())
        .collect();

    String::from_utf8_lossy(&combined_data).into_owned()
}

fn simulate_shortcut_key_sequence(
    keyboard_controller: &mut KeyboardController<KeyboardLayout>,
    letter_key: &str,
) {
    let modifier_key = if cfg!(target_os = "macos") {
        "Meta"
    } else {
        "Control"
    };
    let mut modifier_keystroke = KeystrokeData {
        key: modifier_key.to_string(),
        meta: false,
        shift: false,
        ctrl: false,
        alt: false,
        down: true,
    };
    keyboard_controller.simulate_keystrokes(modifier_keystroke.clone());
    std::thread::sleep(std::time::Duration::from_millis(10));

    let mut keystroke_data = KeystrokeData {
        key: letter_key.to_string(),
        meta: cfg!(target_os = "macos"),
        shift: false,
        ctrl: !cfg!(target_os = "macos"),
        alt: false,
        down: true,
    };
    keyboard_controller.simulate_keystrokes(keystroke_data.clone());
    std::thread::sleep(std::time::Duration::from_millis(10));

    keystroke_data.down = false;
    keyboard_controller.simulate_keystrokes(keystroke_data);
    std::thread::sleep(std::time::Duration::from_millis(10));

    modifier_keystroke.down = false;
    keyboard_controller.simulate_keystrokes(modifier_keystroke);
}

#[derive(Error, Debug)]
pub enum ClipboardError {
    #[error("Failed to create clipboard")]
    CreationError,
}

impl ClipboardController {
    pub fn new() -> Result<Self, ClipboardError> {
        Ok(Self {
            clipboard: arboard::Clipboard::new().map_err(|_| ClipboardError::CreationError)?,
            clipboard_payload: Vec::new(),
        })
    }

    pub fn add_to_clipboard(
        &self,
        is_copy: bool,
        keyboard_controller: &mut KeyboardController<KeyboardLayout>,
    ) {
        let letter_key = if is_copy { "c" } else { "x" };
        simulate_shortcut_key_sequence(keyboard_controller, letter_key);
    }

    pub fn paste_from_clipboard(
        &mut self,
        keyboard_controller: &mut KeyboardController<KeyboardLayout>,
        data: Option<room_service::ClipboardPayload>,
    ) {
        if let Some(packet) = data {
            self.clipboard_payload.push(packet);
            if self.clipboard_payload.len()
                == (self.clipboard_payload.last().unwrap().total_packets as usize)
            {
                let clipboard_data = construct_clipboard_data(&mut self.clipboard_payload);
                match self.clipboard.set_text(clipboard_data) {
                    Ok(_) => {}
                    Err(error) => {
                        log::error!("add_packet: Error setting clipboard text {error:?}");
                    }
                }
                self.clipboard_payload.clear();
            } else {
                // We return early in order to not trigger paste
                log::info!("paste_from_clipboard: Returning early");
                return;
            }
        }
        simulate_shortcut_key_sequence(keyboard_controller, "v");
    }
}
