//! Participant management for remote control sessions.
//!
//! This module provides the Participant and ParticipantsManager types for managing
//! per-participant state including drawing and cursor rendering.

use crate::utils::geometry::Position;
use crate::utils::svg_renderer::SvgRenderError;
use iced::widget::canvas::{Frame, Geometry};
use iced::{Rectangle, Renderer};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[path = "draw.rs"]
mod draw;
use draw::Draw;

#[path = "cursor.rs"]
pub mod cursor;
use cursor::Cursor;

// Re-export CursorMode for external use
pub use cursor::CursorMode;

use crate::room_service::DrawingMode;

const SHARER_COLOR: &str = "#7CCF00";
const DEFAULT_COLOR: &str = "#FF0000";

/// Errors that can occur during participant management.
#[derive(Error, Debug)]
pub enum ParticipantError {
    #[error("Participant already exists: {0}")]
    AlreadyExists(String),
    #[error("SVG render error: {0}")]
    SvgRender(#[from] SvgRenderError),
}

/// Generates a unique visible name based on the full name and existing names.
///
/// # Algorithm
/// - For "John Smith" with no conflicts: returns "John"
/// - If "John" exists: tries "John S", then "John Sm", etc.
/// - If full name exists: adds numbers "John Smith2", "John Smith3", etc.
fn generate_unique_visible_name(name: &str, used_names: &[String]) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let first_name = parts.first().unwrap_or(&name);

    // Try progressively longer candidates
    let candidates = if parts.len() > 1 {
        let last_name = parts[1];
        let mut candidates = vec![first_name.to_string()];

        // Add candidates with increasing characters from last name
        for i in 1..=last_name.chars().count() {
            let partial_last_name: String = last_name.chars().take(i).collect();
            candidates.push(format!("{first_name} {partial_last_name}"));
        }
        candidates
    } else {
        vec![first_name.to_string()]
    };

    // Find first unused candidate
    for candidate in candidates.iter() {
        if !used_names.contains(candidate) {
            return candidate.clone();
        }
    }

    // Fall back to numbering
    let base = candidates.last().unwrap().clone();
    for num in 2.. {
        let candidate = format!("{base}{num}");
        if !used_names.contains(&candidate) {
            return candidate;
        }
    }

    unreachable!()
}

/// Represents a participant in a remote control session.
///
/// Each participant has their own drawing state, cursor, and color.
#[derive(Debug)]
pub struct Participant {
    draw: Draw,
    cursor: Cursor,
    color: &'static str,
}

impl Participant {
    /// Creates a new participant with the given color, name and auto-clear setting.
    ///
    /// # Arguments
    /// * `color` - Hex color string for the participant's drawings and cursor
    /// * `name` - Display name for the participant's cursor
    /// * `auto_clear` - Whether to automatically clear paths after 3 seconds
    pub fn new(color: &'static str, name: &str, auto_clear: bool) -> Result<Self, SvgRenderError> {
        Ok(Self {
            draw: Draw::new(color, auto_clear),
            cursor: Cursor::new(color, name)?,
            color,
        })
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
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Returns a mutable reference to the participant's cursor.
    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// Returns the participant's color.
    pub fn color(&self) -> &str {
        self.color
    }
}

/// Manager that owns Participant objects mapped by participant sid.
///
/// Each participant gets their own Participant instance with their assigned color,
/// drawing state, and cursor.
#[derive(Debug)]
pub struct ParticipantsManager {
    participants: HashMap<String, Participant>,
    /// Available colors for new controllers
    available_colors: VecDeque<&'static str>,
}

impl Default for ParticipantsManager {
    fn default() -> Self {
        Self {
            participants: HashMap::new(),
            available_colors: VecDeque::from([
                "#615FFF", "#009689", "#C800DE", "#00A6F4", "#FFB900", "#ED0040", "#E49500",
                "#B80088", "#FF5BFF", "#00D091",
            ]),
        }
    }
}

impl ParticipantsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a new participant with automatic color assignment.
    ///
    /// # Arguments
    /// * `sid` - Session ID for the participant
    /// * `name` - Full name of the participant (will be made unique)
    /// * `auto_clear` - Whether to automatically clear paths after 3 seconds
    ///
    /// # Returns
    /// The assigned color, or None if no colors are available
    pub fn add_participant(
        &mut self,
        sid: String,
        name: &str,
        auto_clear: bool,
    ) -> Result<(), ParticipantError> {
        // Check if participant already exists
        if self.participants.contains_key(&sid) {
            return Err(ParticipantError::AlreadyExists(sid));
        }

        let color = if sid == "local" {
            SHARER_COLOR
        } else {
            self.available_colors.pop_front().unwrap_or_else(|| {
                log::warn!(
                    "ParticipantsManager::add_participant: no colors available for participant {}",
                    sid
                );
                DEFAULT_COLOR
            })
        };

        let used_names: Vec<String> = self
            .participants
            .values()
            .map(|p| p.cursor().visible_name().to_string())
            .collect();
        let visible_name = generate_unique_visible_name(name, &used_names);

        log::info!(
            "ParticipantsManager::add_participant: sid={} color={} auto_clear={}",
            sid,
            color,
            auto_clear
        );

        self.participants
            .insert(sid, Participant::new(color, &visible_name, auto_clear)?);
        Ok(())
    }

    /// Removes a participant and their data.
    pub fn remove_participant(&mut self, sid: &str) {
        log::info!("ParticipantsManager::remove_participant: sid={}", sid);
        let participant = self.participants.remove(sid);
        if participant.is_none() {
            log::warn!(
                "ParticipantsManager::remove_participant: participant {} not found",
                sid
            );
            return;
        };
        let participant = participant.unwrap();
        if sid != "local" {
            self.available_colors.push_back(participant.color);
        }
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

    /// Sets the cursor mode for a participant.
    pub fn set_cursor_mode(&mut self, sid: &str, mode: CursorMode) {
        if let Some(participant) = self.participants.get_mut(sid) {
            participant.cursor_mut().set_mode(mode);
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
            participant.cursor().draw(&mut in_progress_frame);
        }
        geometries.push(in_progress_frame.into_geometry());

        geometries
    }
}
