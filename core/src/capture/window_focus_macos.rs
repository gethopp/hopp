//! Raise / activate a shareable macOS window so the user can interact with it.

use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use screencapturekit::prelude::*;

/// Brings the application that owns `window_id` (SCWindow / CGWindow id) to the front.
pub fn activate_window(window_id: u64) {
    let Ok(content) = SCShareableContent::get() else {
        log::warn!("activate_window: SCShareableContent::get failed");
        return;
    };
    let Some(window) = content
        .windows()
        .into_iter()
        .find(|window| window.window_id() as u64 == window_id)
    else {
        log::warn!("activate_window: window {window_id} not found");
        return;
    };
    let Some(app) = window.owning_application() else {
        log::warn!("activate_window: no owning application for window {window_id}");
        return;
    };

    let pid = app.process_id();
    let Some(running): Option<Retained<NSRunningApplication>> =
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
    else {
        log::warn!("activate_window: no NSRunningApplication for pid {pid}");
        return;
    };

    let options = NSApplicationActivationOptions::ActivateAllWindows;
    if !running.activateWithOptions(options) {
        log::warn!("activate_window: activateWithOptions failed for pid {pid}");
    } else {
        log::info!("activate_window: activated pid {pid} for window {window_id}");
    }
}
