use std::collections::HashMap;

use crate::KeystrokeData;

#[cfg(target_os = "macos")]
#[path = "keyboard_macos.rs"]
mod platform;

#[cfg(target_os = "windows")]
#[path = "keyboard_windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "keyboard_linux.rs"]
mod platform;

pub use platform::{KeyboardEvent, KeyboardLayout};

// See https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX10.6.sdk/System/Library/Frameworks/Carbon.framework/Versions/A/Frameworks/HIToolbox.framework/Versions/A/Headers/Events.h
pub enum KeyModifier {
    Cmd = 1 << 8,
    Shift = 1 << 9,
    AlphaLock = 1 << 10,
    Option = 1 << 11,
    Ctrl = 1 << 12,
    RightShift = 1 << 13,
    RightOption = 1 << 14,
    RightCtrl = 1 << 15,
}

#[macro_export]
macro_rules! combine_modifiers {
    ($($modifier:ident),*) => {
        {
            let mut result = 0;
            $(
                result |= KeyModifier::$modifier as u32;
            )*
            result
        }
    };
}

pub fn extend_modifier(modifier: u32, new_modifier: KeyModifier) -> u32 {
    modifier | new_modifier as u32
}

pub fn display_mod(modifier: u32) -> String {
    let mut result = String::new();
    if modifier & KeyModifier::Cmd as u32 != 0 {
        result.push_str("Cmd ");
    }
    if modifier & KeyModifier::Shift as u32 != 0 {
        result.push_str("Shift ");
    }
    if modifier & KeyModifier::AlphaLock as u32 != 0 {
        result.push_str("AlphaLock ");
    }
    if modifier & KeyModifier::Option as u32 != 0 {
        result.push_str("Option ");
    }
    if modifier & KeyModifier::Ctrl as u32 != 0 {
        result.push_str("Ctrl ");
    }
    if modifier & KeyModifier::RightShift as u32 != 0 {
        result.push_str("RightShift ");
    }
    if modifier & KeyModifier::RightOption as u32 != 0 {
        result.push_str("RightOption ");
    }
    if modifier & KeyModifier::RightCtrl as u32 != 0 {
        result.push_str("RightCtrl ");
    }
    result
}

pub fn get_modifiers(modifier: u32) -> Vec<KeyModifier> {
    let mut result = Vec::new();
    if modifier & KeyModifier::Cmd as u32 != 0 {
        result.push(KeyModifier::Cmd);
    }
    if modifier & KeyModifier::Shift as u32 != 0 {
        result.push(KeyModifier::Shift);
    }
    if modifier & KeyModifier::AlphaLock as u32 != 0 {
        result.push(KeyModifier::AlphaLock);
    }
    if modifier & KeyModifier::Option as u32 != 0 {
        result.push(KeyModifier::Option);
    }
    if modifier & KeyModifier::Ctrl as u32 != 0 {
        result.push(KeyModifier::Ctrl);
    }
    if modifier & KeyModifier::RightShift as u32 != 0 {
        result.push(KeyModifier::RightShift);
    }
    if modifier & KeyModifier::RightOption as u32 != 0 {
        result.push(KeyModifier::RightOption);
    }
    if modifier & KeyModifier::RightCtrl as u32 != 0 {
        result.push(KeyModifier::RightCtrl);
    }
    result
}

/// A key mapping entry that associates a keycode with a key string and modifier combination.
pub struct KeyMapEntry {
    /// The platform-specific keycode that produces this key combination.
    pub keycode: u16,

    /// The resulting key string when this keycode is pressed with the specified modifiers.
    /// For example: "a", "A", "1", "!", etc.
    pub key: String,

    /// The modifier combination (as a bitmask) that was active when this mapping was created.
    /// Uses the `KeyModifier` enum values combined with bitwise OR operations.
    pub modifiers: u32,
}

pub trait KeyboardEventTrait {
    fn override_utf(&mut self, key: &str);
    fn send(&self);
}

/// Defines the interface for platform-specific keyboard layout operations.
///
/// This trait abstracts keyboard layout functionality across different operating systems,
/// providing a unified interface for key translation, layout change detection, and
/// special key mapping. Each platform (macOS, Windows, Linux) implements this trait
/// to handle platform-specific keyboard APIs and behaviors.
///
/// The trait enables the keyboard simulation system to work consistently across platforms
/// while handling the underlying differences in how each OS manages keyboard layouts,
/// input methods, and key translation.
///
/// # Layout Change Detection
///
/// The trait includes functionality to detect when the user changes their keyboard
/// layout (e.g., switching from QWERTY to DVORAK, or changing language layouts).
/// This is crucial for maintaining accurate key mapping tables.
pub trait KeyboardLayoutTrait {
    /// Translates a platform-specific keycode and modifier combination into a Unicode string.
    ///
    /// This is the core method for understanding what character or string a keycode
    /// produces when pressed with specific modifiers. The translation respects the
    /// current keyboard layout, input method, and dead key states.
    ///
    /// # Arguments
    ///
    /// * `keycode` - Platform-specific virtual keycode (typically 0-127 range)
    /// * `modifier` - Bitmask of active modifiers (see `KeyModifier` enum)
    ///
    /// # Returns
    ///
    /// * `Some(String)` - The Unicode string this keycode+modifier produces
    /// * `None` - If the keycode doesn't produce a character, is invalid, or translation fails
    fn key_translate(&self, keycode: u16, modifier: u32) -> Option<String>;

    /// Checks if the keyboard layout has changed since the last call to this method.
    ///
    /// This method detects when the user switches keyboard layouts, input methods,
    /// or language settings. It's essential for maintaining accurate key mapping
    /// because different layouts can produce completely different characters for
    /// the same keycode.
    ///
    /// The method is stateful - it tracks layout changes internally and resets
    /// the "changed" flag after each call, so calling it multiple times in
    /// succession will only return `true` on the first call after a change.
    ///
    /// # State Management
    ///
    /// - **First call after change**: Returns `true` and resets change flag
    /// - **Subsequent calls**: Return `false` until next layout change
    /// - **Thread safety**: Implementation should handle concurrent access appropriately
    ///
    /// # Returns
    ///
    /// * `true` - Layout has changed since last call, key maps should be rebuilt
    /// * `false` - No layout change detected
    fn has_changed(&mut self) -> bool;

    /// Returns a map of layout-independent special keys to their platform-specific keycodes.
    ///
    /// Layout-independent keys are special keys that maintain consistent keycodes
    /// across different keyboard layouts. These include navigation keys, function
    /// keys, and other non-character keys that don't change based on language
    /// or layout selection.
    ///
    /// This map provides a fast lookup for keys that don't need layout-specific
    /// translation, bypassing the more expensive `key_translate` process for
    /// these common keys.
    /// # Returns
    ///
    /// A HashMap mapping standardized key names to platform-specific keycodes.
    /// The keys are static string references for efficiency.
    fn get_independent_codes(&self) -> HashMap<&'static str, u16>;
}

/// A comprehensive key mapping table that translates key strings and modifier combinations
/// to platform-specific keycodes.
///
/// The `KeyMap` struct builds a lookup table during initialization by querying the keyboard
/// layout for all possible keycode and modifier combinations. This allows for efficient
/// reverse lookups when simulating keystrokes - given a key string and modifiers, it can
/// quickly find the corresponding keycode.
///
/// The mapping handles two types of keys:
/// - **Layout-dependent keys**: Regular character keys that vary based on keyboard layout
/// - **Layout-independent keys**: Special keys (arrows, function keys, etc.) that have
///   consistent keycodes across layouts
struct KeyMap {
    /// Vector of all discovered key mappings from the keyboard layout.
    /// Each entry contains a keycode, the resulting key string, and the modifier state.
    entries: Vec<KeyMapEntry>,

    /// Map of layout-independent keys (like arrows, function keys) to their keycodes.
    /// These keys maintain consistent keycodes regardless of the keyboard layout.
    independent_codes: HashMap<&'static str, u16>,
}

impl KeyMap {
    /// Creates a new `KeyMap` by building a comprehensive lookup table from the given keyboard layout.
    ///
    /// This method systematically queries the keyboard layout for all combinations of:
    /// - Keycodes 0-127 (covering the standard key range)
    /// - Common modifier combinations (None, Shift, Option, Cmd, and their combinations)
    ///
    /// The resulting entries are stored for fast reverse lookup during keystroke simulation.
    /// Layout-independent keys are also cached from the keyboard layout.
    ///
    /// # Arguments
    ///
    /// * `layout` - A keyboard layout that implements `KeyboardLayoutTrait`
    ///
    /// # Returns
    ///
    /// A new `KeyMap` instance with populated lookup tables.
    fn new(layout: &impl KeyboardLayoutTrait) -> Self {
        let modifiers = vec![
            0,
            combine_modifiers!(Shift),
            combine_modifiers!(Option),
            combine_modifiers!(Cmd),
            combine_modifiers!(Shift, Option),
            combine_modifiers!(Shift, Cmd),
            combine_modifiers!(Option, Cmd),
        ];
        let mut entries = Vec::new();
        for modifier in modifiers {
            for i in 0..128 {
                let result = layout.key_translate(i, modifier);
                if let Some(s) = result {
                    log::debug!(
                        "Keymap entry: code {} key {} modifiers {}",
                        i,
                        s,
                        display_mod(modifier)
                    );
                    entries.push(KeyMapEntry {
                        keycode: i,
                        key: s,
                        modifiers: modifier,
                    });
                }
            }
        }
        let independent_codes = layout.get_independent_codes();
        Self {
            entries,
            independent_codes,
        }
    }

    /// Finds the keycode for a given key string and modifier combination.
    ///
    /// This method performs a reverse lookup to find the keycode that would produce
    /// the specified key when pressed with the given modifiers. It handles both
    /// layout-independent keys (which have fixed keycodes) and layout-dependent
    /// keys (which are looked up in the entries table).
    ///
    /// # Special Handling
    ///
    /// - **Layout-independent keys**: Checked first in the `independent_codes` map
    /// - **Ctrl modifier**: When Ctrl is pressed, it's filtered out during lookup
    ///   because Ctrl+key combinations typically don't produce visible characters
    ///   (except for numbers), making the translation table less useful
    ///
    /// # Arguments
    ///
    /// * `key` - The key string to find (e.g., "a", "A", "Enter", "ArrowUp")
    /// * `modifier` - The modifier combination as a bitmask (see `KeyModifier` enum)
    ///
    /// # Returns
    ///
    /// * `Some(keycode)` - The keycode that produces this key with these modifiers
    /// * `None` - If no matching keycode is found
    pub fn get_code(&self, key: &str, mut modifier: u32) -> Option<u16> {
        let code = self.independent_codes.get(key);
        if code.is_some() {
            return code.cloned();
        }

        /*
         * When the Ctrl key is pressed the key codes are not producing
         * key characters except from numbers. Therefore we disable the
         * modifier when searching as the translation table doesn't provide
         * any useful information.
         */
        if modifier & KeyModifier::Ctrl as u32 != 0 {
            modifier &= !(KeyModifier::Ctrl as u32);
        }

        for entry in self.entries.iter() {
            if entry.key == key && entry.modifiers == modifier {
                return Some(entry.keycode);
            }
        }

        None
    }
}

/// High-level controller for keyboard input simulation across platforms.
///
/// The `KeyboardController` orchestrates keyboard simulation by managing layout detection,
/// key mapping, and event generation. It automatically handles layout changes and provides
/// a simple interface for simulating keystrokes from high-level keystroke data.
///
/// The controller maintains an internal key mapping table that's rebuilt when the keyboard
/// layout changes, ensuring accurate key simulation regardless of the user's current
/// input method or language settings.
///
/// # Type Parameter
///
/// * `T` - The keyboard layout implementation (must implement `KeyboardLayoutTrait`)
///
/// # Features
///
/// - Automatic layout change detection and key map rebuilding
/// - Cross-platform keystroke simulation
/// - Support for modifier keys (Shift, Ctrl, Alt/Option, Cmd/Meta)
/// - Layout-independent special key handling
/// - UTF string override for cross-layout compatibility
pub struct KeyboardController<T: KeyboardLayoutTrait> {
    /// Internal key mapping table for efficient keycode lookups.
    map: KeyMap,
    /// Platform-specific keyboard layout handler.
    layout: T,
    /// Whether keyboard simulation is currently enabled.
    enabled: bool,
}

impl<T: KeyboardLayoutTrait> KeyboardController<T> {
    /// Creates a new keyboard controller with the default platform layout.
    ///
    /// This initializes the controller with a fresh keyboard layout and builds
    /// the initial key mapping table by querying all possible keycode and
    /// modifier combinations.
    ///
    /// # Returns
    ///
    /// A new `KeyboardController` instance ready for keystroke simulation.
    pub fn new() -> KeyboardController<KeyboardLayout> {
        let layout = KeyboardLayout::new();
        let map = KeyMap::new(&layout);
        KeyboardController {
            map,
            layout,
            enabled: true,
        }
    }

    /// Enables or disables keyboard simulation.
    ///
    /// When disabled, calls to `simulate_keystrokes` will be ignored.
    /// This is useful for temporarily suspending keyboard simulation
    /// without destroying the controller state.
    ///
    /// # Arguments
    ///
    /// * `enabled` - `true` to enable simulation, `false` to disable
    pub fn set_enabled(&mut self, enabled: bool) {
        log::info!("set_enabled: {enabled}");
        self.enabled = enabled;
    }

    /// Simulates a keystroke from high-level keystroke data.
    ///
    /// This method translates the provided keystroke data into platform-specific
    /// key events and sends them to the system. It handles modifier key mapping,
    /// layout change detection, and UTF string override for cross-layout compatibility.
    ///
    /// # Key Processing Steps
    ///
    /// 1. Check if simulation is enabled (early return if disabled)
    /// 2. Convert boolean modifiers to platform modifier bitmask
    /// 3. Detect and handle layout changes (rebuild key map if needed)
    /// 4. Look up keycode for the key + modifier combination
    /// 5. Create platform-specific keyboard event
    /// 6. Override UTF string for layout-independent character input
    /// 7. Send the event to the system
    ///
    /// # UTF Override Logic
    ///
    /// For visible character keys (excluding special keys like arrows, function keys),
    /// the method overrides the platform's character translation with the original
    /// key string. This ensures proper cross-layout compatibility when the sender
    /// and receiver use different keyboard layouts.
    ///
    /// UTF override is skipped when:
    /// - Meta or Ctrl modifiers are active (typically non-character shortcuts)
    /// - Key is layout-independent (Enter, Tab, arrows, etc.)
    /// - Keystroke is a key release event (`down = false`)
    ///
    /// # Arguments
    ///
    /// * `keystroke_data` - High-level keystroke information including key, modifiers, and press state
    ///
    /// In case of error this function simply does nothing because we don't want to
    /// kill the session if a button is not working.
    pub fn simulate_keystrokes(&mut self, keystroke_data: KeystrokeData) {
        log::debug!("simulate_keystrokes: key: {keystroke_data:?}");
        if !self.enabled {
            return;
        }

        let mut modifier = 0;
        if keystroke_data.shift {
            modifier = extend_modifier(modifier, KeyModifier::Shift);
        }
        if keystroke_data.meta {
            modifier = extend_modifier(modifier, KeyModifier::Cmd);
        }
        if keystroke_data.alt {
            modifier = extend_modifier(modifier, KeyModifier::Option);
        }
        if keystroke_data.ctrl {
            modifier = extend_modifier(modifier, KeyModifier::Ctrl);
        }

        let layout_changed = self.layout.has_changed();
        if layout_changed {
            log::info!("simulate_keystrokes: layout changed updating map");
            self.map = KeyMap::new(&self.layout);
        }

        let keycode = match self.map.get_code(&keystroke_data.key, modifier) {
            Some(keycode) => keycode,
            None => {
                log::warn!(
                    "simulate_keystrokes: failed to get keycode for key: {}",
                    keystroke_data.key
                );
                0
            }
        };

        let event = KeyboardEvent::new(keycode, modifier, keystroke_data.down);
        if event.is_none() {
            log::error!("simulate_keystrokes: couldn't create keyboard event");
            return;
        }
        let mut event = event.unwrap();

        /*
         * We only overwrite the utf string for non layout independent keys
         * in order to handle where the case where the sharer is using different
         * keyboard layout from the viewer.
         *
         * When cmd or ctrl are pressed no visible characters are inserted.
         */
        if (keystroke_data.key != "Enter")
            && (keystroke_data.key != "Tab")
            && (keystroke_data.key != "Backspace")
            && (keystroke_data.key != "Escape")
            && (keystroke_data.key != "Delete")
            && (keystroke_data.key != "ArrowLeft")
            && (keystroke_data.key != "ArrowRight")
            && (keystroke_data.key != "ArrowUp")
            && (keystroke_data.key != "ArrowDown")
            && (keystroke_data.key != "PageUp")
            && (keystroke_data.key != "PageDown")
            && (keystroke_data.key != "Control")
            && (keystroke_data.key != "Shift")
            && (!keystroke_data.key.is_empty())
            && !keystroke_data.meta
            && !keystroke_data.ctrl
            && keystroke_data.down
        {
            event.override_utf(&keystroke_data.key);
        }

        event.send();
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    #[test]
    fn test_keyboard_simulator() {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        let _ = KeyboardController::<KeyboardLayout>::new();
    }
}
