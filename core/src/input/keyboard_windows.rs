use super::{get_modifiers, KeyModifier, KeyboardEventTrait, KeyboardLayoutTrait};

use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardState, MapVirtualKeyExW, SendInput, ToUnicode, HKL, INPUT,
        INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
        VK_ESCAPE, VK_HOME, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN,
        VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_SHIFT, VK_TAB, VK_UP,
    },
    WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use std::collections::HashMap;

fn map_modifier_to_virtual_key(modifier: KeyModifier) -> u32 {
    let virtual_key = match modifier {
        KeyModifier::Cmd => VK_LWIN,
        KeyModifier::Shift => VK_SHIFT,
        KeyModifier::AlphaLock => VK_CAPITAL,
        KeyModifier::Option => VK_MENU,
        KeyModifier::Ctrl => VK_CONTROL,
        KeyModifier::RightShift => VK_RSHIFT,
        KeyModifier::RightOption => VK_RMENU,
        KeyModifier::RightCtrl => VK_RCONTROL,
    };
    virtual_key.0 as u32
}

pub struct KeyboardLayout {
    layout: HKL,
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardLayout {
    pub fn new() -> Self {
        let layout = unsafe {
            let thread_id = GetWindowThreadProcessId(GetForegroundWindow(), None);
            GetKeyboardLayout(thread_id)
        };
        Self { layout }
    }
}

impl KeyboardLayoutTrait for KeyboardLayout {
    fn key_translate(&self, keycode: u16, modifiers: u32) -> Option<String> {
        let scan_code = unsafe { MapVirtualKeyExW(keycode as u32, MAPVK_VK_TO_VSC, self.layout) };
        if scan_code == 0 {
            return None;
        }
        let mut keyboard_state = [0u8; 256];
        unsafe {
            let res = GetKeyboardState(&mut keyboard_state);
            if let Err(e) = res {
                log::error!("key_translate GetKeyboardState failed {e:?}");
                return None;
            }
        }

        let modifiers = get_modifiers(modifiers);
        for modifier in modifiers {
            let virtual_key = map_modifier_to_virtual_key(modifier);
            if virtual_key != 0 {
                keyboard_state[virtual_key as usize] |= 0x80;
            }
        }

        let mut res: [u16; 4] = [0; 4];
        let unicode = unsafe {
            ToUnicode(
                keycode as u32,
                scan_code,
                Some(&keyboard_state),
                &mut res,
                0,
            )
        };
        if unicode <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&res[..unicode as usize]))
    }

    fn has_changed(&mut self) -> bool {
        let layout = unsafe {
            let thread_id = GetWindowThreadProcessId(GetForegroundWindow(), None);
            GetKeyboardLayout(thread_id)
        };
        let has_changed = layout != self.layout;
        if has_changed {
            self.layout = layout;
        }
        has_changed
    }

    fn get_independent_codes(&self) -> HashMap<&'static str, u16> {
        let mut map: HashMap<&'static str, u16> = HashMap::new();
        map.insert("Enter", VK_RETURN.0);
        map.insert("Tab", VK_TAB.0);
        map.insert("Backspace", VK_BACK.0);
        map.insert("Escape", VK_ESCAPE.0);
        map.insert("ArrowUp", VK_UP.0);
        map.insert("ArrowDown", VK_DOWN.0);
        map.insert("ArrowLeft", VK_LEFT.0);
        map.insert("ArrowRight", VK_RIGHT.0);
        map.insert("PageUp", VK_PRIOR.0);
        map.insert("PageDown", VK_NEXT.0);
        map.insert("Home", VK_HOME.0);
        map.insert("End", VK_END.0);
        map.insert("Delete", VK_DELETE.0);
        map.insert("Control", VK_CONTROL.0);
        map.insert("Shift", VK_SHIFT.0);
        map.insert("Meta", VK_LWIN.0);
        map.insert("Alt", VK_MENU.0);
        map.insert("CapsLock", VK_CAPITAL.0);
        map
    }
}

fn keyboard_input(keycode: u16, unicode: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(keycode),
                wScan: unicode,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_input(input: &[INPUT]) {
    let input_size = std::mem::size_of::<INPUT>() as i32;
    let input_len = input.len() as u32;
    let result = unsafe { SendInput(input, input_size) };
    if result != input_len {
        log::error!("keyboard_input: SendInput failed: {result:?}");
    }
}

pub struct KeyboardEvent {
    flags: KEYBD_EVENT_FLAGS,
    unicode: u16,
    keycode: u16,
}

impl KeyboardEvent {
    pub fn new(keycode: u16, _modifier: u32, down: bool) -> Option<Self> {
        let flags = if down {
            KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };
        Some(Self {
            keycode,
            unicode: 0,
            flags,
        })
    }
}

impl KeyboardEventTrait for KeyboardEvent {
    fn override_utf(&mut self, key: &str) {
        self.unicode = match key.encode_utf16().next() {
            Some(unicode) => unicode,
            None => {
                log::error!("override_utf: key: {key} is not a valid unicode");
                sentry_utils::upload_logs_event("KeyboardEvent override_utf failed".to_string());
                0
            }
        };
        self.flags |= KEYEVENTF_UNICODE;
        self.keycode = 0;
    }

    fn send(&self) {
        let input = keyboard_input(self.keycode, self.unicode, self.flags);
        let inputs = vec![input];
        send_input(&inputs);
    }
}
