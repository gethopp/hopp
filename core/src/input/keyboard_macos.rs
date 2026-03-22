#![allow(non_snake_case)]
use std::os::raw::c_void;

use super::{get_modifiers, KeyModifier, KeyboardEventTrait, KeyboardLayoutTrait};

use core_foundation::{
    base::{OSStatus, TCFType},
    data::{CFData, CFDataGetBytePtr, CFDataRef},
    dictionary::CFDictionaryRef,
    string::CFStringRef,
};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use internal::{CFNotificationCenterRef, TISInputSourceRef};

use std::collections::HashMap;
mod internal {
    #![allow(non_snake_case)]
    use std::os::raw::c_void;

    #[repr(C)]
    pub struct __TISInputSourceRef(c_void);
    pub type TISInputSourceRef = *const __TISInputSourceRef;

    #[repr(C)]
    pub struct __CFNotificationCenterRef(c_void);
    pub type CFNotificationCenterRef = *const __CFNotificationCenterRef;
}

pub type CFNotificationCallback = Option<
    extern "C" fn(
        center: CFNotificationCenterRef,
        observer: *mut c_void,
        name: CFStringRef,
        object: *const c_void,
        userInfo: CFDictionaryRef,
    ),
>;

extern "C" {
    #[allow(non_upper_case_globals)]
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    //static kTISNotifySelectcallbackedKeyboardInputSourceChanged: CFStringRef;
    pub static kTISNotifySelectedKeyboardInputSourceChanged: CFStringRef;

    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(source: TISInputSourceRef, propertyKey: CFStringRef) -> CFDataRef;
    fn UCKeyTranslate(
        keyLayoutPtr: *const c_void,
        virtualKeyCode: u16,
        keyAction: u16,
        modifierKeyState: u32,
        keyboardType: u32,
        keyTranslateOptions: u32,
        deadKeyState: *mut u32,
        maxStringLength: u32,
        actualStringLength: *mut u32,
        unicodeString: *mut u16,
    ) -> OSStatus;

    fn LMGetKbdLast() -> u8;

    pub fn CFNotificationCenterGetDistributedCenter() -> CFNotificationCenterRef;
    pub fn CFNotificationCenterAddObserver(
        center: CFNotificationCenterRef,
        observer: *const c_void,
        callBack: CFNotificationCallback,
        name: CFStringRef,
        object: *const c_void,
        suspensionBehavior: u32,
    );
    pub fn CFNotificationCenterRemoveObserver(
        center: CFNotificationCenterRef,
        observer: *const c_void,
        name: CFStringRef,
        object: *const c_void,
    );

    //pub fn CFRunLoopRun();
}

pub struct KeyboardLayout {
    data: CFData,
    changed: Box<bool>,
}

fn get_layout_data() -> CFData {
    unsafe {
        let mut source = TISCopyCurrentKeyboardInputSource();
        let mut data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        // See https://github.com/microsoft/node-native-keymap/blob/main/src/keyboard_mac.mm
        if data.is_null() {
            source = TISCopyCurrentKeyboardLayoutInputSource();
            data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        }
        if data.is_null() {
            log::error!("Failed to get keyboard layout data");
        }
        CFData::wrap_under_get_rule(data)
    }
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardLayout {
    pub fn new() -> Self {
        let data = get_layout_data();
        let changed = Box::new(false);

        unsafe {
            let center = CFNotificationCenterGetDistributedCenter();
            CFNotificationCenterAddObserver(
                center,
                &*changed as *const _ as *mut c_void,
                Some(observer),
                kTISNotifySelectedKeyboardInputSourceChanged,
                std::ptr::null_mut(),
                4,
            );
        }
        Self { data, changed }
    }
}

impl KeyboardLayoutTrait for KeyboardLayout {
    fn key_translate(&self, keycode: u16, modifier: u32) -> Option<String> {
        unsafe {
            let mut keys_downs: u32 = 0;
            let mut chars: [u16; 1] = [0];
            let mut real_length: u32 = 0;
            let result = UCKeyTranslate(
                CFDataGetBytePtr(self.data.as_CFTypeRef() as CFDataRef) as *const c_void,
                keycode,
                3, // kUCKeyActionDisplay
                modifier,
                LMGetKbdLast() as u32,
                0,
                &mut keys_downs,
                1,
                &mut real_length,
                chars.as_mut_ptr(),
            );
            if result != 0 {
                log::error!("UCKeyTranslate failed with error code {result}");
                return None;
            }
            let utf16 = &chars[0..real_length as usize];
            match String::from_utf16(utf16) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::error!("Failed to convert utf16 to string: {e}");
                    None
                }
            }
        }
    }

    fn has_changed(&mut self) -> bool {
        let changed = *self.changed;
        if changed {
            self.data = get_layout_data();
            *self.changed = false;
        }
        changed
    }

    fn get_independent_codes(&self) -> HashMap<&'static str, u16> {
        let mut independent_codes: HashMap<&str, u16> = HashMap::new();
        independent_codes.insert("Enter", 0x24);
        independent_codes.insert("Tab", 0x30);
        independent_codes.insert("Backspace", 0x33);
        independent_codes.insert("Escape", 0x35);
        independent_codes.insert("ArrowUp", 0x7E);
        independent_codes.insert("ArrowDown", 0x7D);
        independent_codes.insert("ArrowLeft", 0x7B);
        independent_codes.insert("ArrowRight", 0x7C);
        independent_codes.insert("PageUp", 0x74);
        independent_codes.insert("PageDown", 0x79);
        independent_codes.insert("Home", 0x73);
        independent_codes.insert("End", 0x77);
        independent_codes.insert("Delete", 0x75);
        independent_codes.insert("Control", 0x3B);
        independent_codes.insert("Shift", 0x38);
        independent_codes.insert("Meta", 0x37);
        independent_codes.insert("Alt", 0x3A);
        independent_codes.insert("CapsLock", 0x39);
        independent_codes
    }
}

extern "C" fn observer(
    _center: CFNotificationCenterRef,
    observer: *mut c_void,
    _name: CFStringRef,
    _object: *const c_void,
    _user_info: CFDictionaryRef,
) {
    log::info!("observer: Keyboard layout changed");
    let changed_flag = unsafe { &mut *(observer as *mut bool) };
    *changed_flag = true;
}

impl Drop for KeyboardLayout {
    fn drop(&mut self) {
        unsafe {
            let center = CFNotificationCenterGetDistributedCenter();
            CFNotificationCenterRemoveObserver(
                center,
                &*self.changed as *const _ as *mut c_void,
                kTISNotifySelectedKeyboardInputSourceChanged,
                std::ptr::null(),
            );
        }
    }
}

pub struct KeyboardEvent {
    event: CGEvent,
}

impl KeyboardEvent {
    pub fn new(keycode: u16, modifier: u32, down: bool) -> Option<Self> {
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(()) => {
                log::error!("simulate_keystrokes: Failed to create event source");
                return None;
            }
        };

        let event = match CGEvent::new_keyboard_event(event_source, keycode, down) {
            Ok(event) => event,
            Err(()) => {
                log::error!("simulate_keystrokes: Failed to create keyboard even ",);
                return None;
            }
        };

        let mut event_flags = CGEventFlags::empty();
        let modifiers = get_modifiers(modifier);
        for modif in modifiers {
            match modif {
                KeyModifier::Shift => event_flags.insert(CGEventFlags::CGEventFlagShift),
                KeyModifier::Cmd => event_flags.insert(CGEventFlags::CGEventFlagCommand),
                KeyModifier::Option => event_flags.insert(CGEventFlags::CGEventFlagAlternate),
                KeyModifier::Ctrl => event_flags.insert(CGEventFlags::CGEventFlagControl),
                _ => {}
            }
        }
        event.set_flags(event_flags);

        Some(Self { event })
    }
}

impl KeyboardEventTrait for KeyboardEvent {
    fn override_utf(&mut self, key: &str) {
        let utf_16: Vec<u16> = key.encode_utf16().collect();
        log::debug!("simulate_keystrokes: overwritten utf char: {utf_16:?}");
        self.event.set_string_from_utf16_unchecked(&utf_16);
    }

    fn send(&self) {
        self.event.post(CGEventTapLocation::HID);
    }
}
