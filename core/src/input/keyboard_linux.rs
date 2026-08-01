#![allow(non_snake_case)]
use std::collections::HashMap;

use super::{KeyboardEventTrait, KeyboardLayoutTrait};

pub struct KeyboardLayout {}

impl KeyboardLayout {
    pub fn new() -> Self {
        Self {}
    }
}

impl KeyboardLayoutTrait for KeyboardLayout {
    fn key_translate(&self, keycode: u16, modifier: u32) -> Option<String> {
        None
    }

    fn has_changed(&mut self) -> bool {
        false
    }

    fn get_independent_codes(&self) -> HashMap<&'static str, u16> {
        HashMap::new()
    }
}

pub struct KeyboardEvent {}

impl KeyboardEvent {
    pub fn new(
        keycode: u16,
        modifier: u32,
        down: bool,
        _target_process_id: Option<i32>,
    ) -> Option<Self> {
        None
    }
}

impl KeyboardEventTrait for KeyboardEvent {
    fn override_utf(&mut self, key: &str) {}

    fn send(&self) {}
}
