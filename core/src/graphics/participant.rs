//! Participant management for remote control sessions.
//!
//! This module provides the Participant and ParticipantsManager types for managing
//! per-participant state including drawing and cursor rendering.

use crate::utils::geometry::Position;
use iced::widget::canvas::{Frame, Geometry};
use iced::{Rectangle, Renderer};
use std::collections::HashMap;

#[path = "draw.rs"]
mod draw;
use draw::Draw;

#[path = "iced_cursor.rs"]
mod iced_cursor;
use iced_cursor::IcedCursor;

use crate::room_service::DrawingMode;

/// Stub function to resolve participant name.
/// TODO: Replace with actual name resolution logic.
fn resolve_name() -> String {
    "User".to_string()
}

/// Represents a participant in a remote control session.
///
/// Each participant has their own drawing state, cursor, and color.
#[derive(Debug)]
pub struct Participant {
    draw: Draw,
    cursor: IcedCursor,
    color: String,
}

impl Participant {
    /// Creates a new participant with the given color and auto-clear setting.
    ///
    /// # Arguments
    /// * `color` - Hex color string for the participant's drawings and cursor
    /// * `auto_clear` - Whether to automatically clear paths after 3 seconds
    pub fn new(color: &str, auto_clear: bool) -> Self {
        let name = resolve_name();
        Self {
            draw: Draw::new(color, auto_clear),
            cursor: IcedCursor::new(color, &name),
            color: color.to_string(),
        }
    }

    /// Returns a reference to the participant's Draw instance.
    pub fn draw(&self) -> &Draw {
        &self.draw
    }

    /// Returns a mutable reference to the participant's Draw instance.
    pub fn draw_mut(&mut self) -> &mut Draw {
        &mut self.draw
    }

    /// Returns a reference to the participant's cursor.
    pub fn cursor(&self) -> &IcedCursor {
        &self.cursor
    }

    /// Returns a mutable reference to the participant's cursor.
    pub fn cursor_mut(&mut self) -> &mut IcedCursor {
        &mut self.cursor
    }

    /// Returns the participant's color.
    pub fn color(&self) -> &str {
        &self.color
    }
}

/// Manager that owns Participant objects mapped by participant sid.
///
/// Each participant gets their own Participant instance with their assigned color,
/// drawing state, and cursor.
#[derive(Default, Debug)]
pub struct ParticipantsManager {
    participants: HashMap<String, Participant>,
}

impl ParticipantsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a new participant with their color.
    pub fn add_participant(&mut self, sid: String, color: &str, auto_clear: bool) {
        log::info!(
            "ParticipantsManager::add_participant: sid={} color={} auto_clear={}",
            sid,
            color,
            auto_clear
        );
        self.participants
            .insert(sid, Participant::new(color, auto_clear));
    }

    /// Removes a participant and their data.
    pub fn remove_participant(&mut self, sid: &str) {
        log::info!("ParticipantsManager::remove_participant: sid={}", sid);
        self.participants.remove(sid);
    }

    /// Sets the drawing mode for a specific participant.
    pub fn set_drawing_mode(&mut self, sid: &str, mode: DrawingMode) {
        log::debug!(
            "ParticipantsManager::set_drawing_mode: sid={} mode={:?}",
            sid,
            mode
        );
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().set_mode(mode);
        } else {
            log::warn!(
                "ParticipantsManager::set_drawing_mode: participant {} not found",
                sid
            );
        }
    }

    /// Starts a new drawing path for a participant.
    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        log::debug!(
            "ParticipantsManager::draw_start: sid={} point={:?} path_id={}",
            sid,
            point,
            path_id
        );
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().start_path(path_id, point);
        } else {
            log::warn!(
                "ParticipantsManager::draw_start: participant {} not found",
                sid
            );
        }
    }

    /// Adds a point to the current drawing path for a participant.
    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        log::debug!(
            "ParticipantsManager::draw_add_point: sid={} point={:?}",
            sid,
            point
        );
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().add_point(point);
        } else {
            log::warn!(
                "ParticipantsManager::draw_add_point: participant {} not found",
                sid
            );
        }
    }

    /// Ends the current drawing path for a participant.
    pub fn draw_end(&mut self, sid: &str, point: Position) {
        log::debug!(
            "ParticipantsManager::draw_end: sid={} point={:?}",
            sid,
            point
        );
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().add_point(point);
            participant.draw_mut().finish_path();
        } else {
            log::warn!(
                "ParticipantsManager::draw_end: participant {} not found",
                sid
            );
        }
    }

    /// Clears a specific drawing path for a participant.
    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        log::debug!(
            "ParticipantsManager::draw_clear_path: sid={} path_id={}",
            sid,
            path_id
        );
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().clear_path(path_id);
        } else {
            log::warn!(
                "ParticipantsManager::draw_clear_path: participant {} not found",
                sid
            );
        }
    }

    /// Clears all drawing paths for a participant.
    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        log::info!("ParticipantsManager::draw_clear_all_paths: sid={}", sid);
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.draw_mut().clear();
        } else {
            log::warn!(
                "ParticipantsManager::draw_clear_all_paths: participant {} not found",
                sid
            );
        }
    }

    /// Updates auto-clear for all participants and returns removed path IDs.
    ///
    /// This should be called periodically to expire old paths for participants
    /// with auto_clear enabled.
    ///
    /// # Returns
    /// A vector of removed path IDs
    pub fn update_auto_clear(&mut self) -> Vec<u64> {
        let mut removed_path_ids = Vec::new();
        for participant in self.participants.values_mut() {
            removed_path_ids.extend(participant.draw_mut().clear_expired_paths());
        }
        removed_path_ids
    }

    /// Sets the cursor position for a specific participant.
    pub fn set_cursor_position(&mut self, sid: &str, position: Option<Position>) {
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.cursor_mut().set_position(position);
        }
    }

    /// Renders all participants' drawings and cursors.
    ///
    /// # Returns
    /// A vector of Geometry objects representing all rendered content
    pub fn draw(&self, renderer: &Renderer, bounds: Rectangle) -> Vec<Geometry> {
        let mut geometries = Vec::with_capacity(self.participants.len() + 1);

        // Collect cached completed geometries from each participant's Draw
        for participant in self.participants.values() {
            geometries.push(participant.draw().draw_completed(renderer, bounds));
        }

        // Draw all in-progress paths into a single frame
        let mut in_progress_frame = Frame::new(renderer, bounds.size());
        for participant in self.participants.values() {
            participant
                .draw()
                .draw_in_progress_to_frame(&mut in_progress_frame);
        }

        for participant in self.participants.values() {
            // Draw cursor with pointer=false (normal cursor for now)
            participant.cursor().draw(&mut in_progress_frame, false);
        }
        geometries.push(in_progress_frame.into_geometry());

        geometries
    }
}
