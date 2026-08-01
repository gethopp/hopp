#![allow(non_snake_case)]

use super::CursorSimulatorFunctions;
use crate::{input::mouse::SharerCursor, utils::geometry::Position, MouseClickData, ScrollDelta};

use crate::overlay_window::OverlayWindow;

use winit::dpi::PhysicalPosition;

use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub struct MouseObserver {}

impl MouseObserver {
    pub fn new(internal: Arc<Mutex<SharerCursor>>) -> Result<Self, ()> {
        Ok(Self {})
    }
}

pub struct CursorSimulator {}

impl CursorSimulator {
    pub fn new() -> Self {
        Self {}
    }

    /* macOS only: no conversion measurement is cached on this platform. */
    pub fn invalidate_sender_flip_height(&mut self) {}

    /* macOS only: pinned window delivery does not exist on this platform. */
    pub fn has_window_target(&self) -> bool {
        false
    }
}

impl CursorSimulatorFunctions for CursorSimulator {
    fn simulate_cursor_movement(&mut self, position: Position, click_down: bool) {
        log::error!("default_observer.rs: simulate_cursor_movement");
    }
    fn simulate_click(&mut self, click_data: MouseClickData) {
        log::error!("default_observer.rs: simulate_click");
    }
    fn simulate_scroll(&mut self, delta: ScrollDelta) {
        log::error!("default_observer.rs: simulate_scroll");
    }
}
