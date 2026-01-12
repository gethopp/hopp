use std::collections::HashMap;

use iced::widget::canvas::{path, stroke, Cache, Frame, Geometry, Stroke};
use iced::{Color, Point, Rectangle, Renderer};

use crate::{room_service::DrawingMode, utils::geometry::Position};

fn color_from_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');

    // Check if the hex string has at least 6 characters to avoid panic
    if hex.len() < 6 {
        log::warn!(
            "color_from_hex: invalid hex color '{}', using default black color",
            hex
        );
        return Color::from_rgb8(0, 0, 0);
    }

    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    Color::from_rgb8(r, g, b)
}

#[derive(Debug, Clone)]
struct DrawPath {
    path_id: u64,
    points: Vec<Position>,
}

impl DrawPath {
    pub fn new(path_id: u64, point: Position) -> Self {
        Self {
            path_id,
            points: vec![point],
        }
    }
}

pub struct Draw {
    in_progress_path: Option<DrawPath>,
    completed_paths: Vec<DrawPath>,
    completed_cache: Cache,
    mode: DrawingMode,
    color: Color,
}

impl std::fmt::Debug for Draw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Draw")
            .field("in_progress_path", &self.in_progress_path)
            .field("completed_paths", &self.completed_paths)
            .field("mode", &self.mode)
            .field("color", &self.color)
            .finish()
    }
}

impl Draw {
    pub fn new(color: &str) -> Self {
        Self {
            in_progress_path: None,
            completed_paths: Vec::new(),
            completed_cache: Cache::new(),
            mode: DrawingMode::Disabled,
            color: color_from_hex(color),
        }
    }

    pub fn set_mode(&mut self, mode: DrawingMode) {
        self.mode = mode.clone();
        if mode == DrawingMode::Disabled {
            self.clear();
        }
    }

    pub fn start_path(&mut self, path_id: u64, point: Position) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("start_path: drawing mode is disabled, skipping path");
            return;
        }

        log::info!("start_path: starting new path with id {}", path_id);
        self.in_progress_path = Some(DrawPath::new(path_id, point));
    }

    pub fn add_point(&mut self, point: Position) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("add_point: drawing mode is disabled, skipping point");
            return;
        }

        if let Some(in_progress_path) = self.in_progress_path.as_mut() {
            in_progress_path.points.push(point);
        } else {
            log::warn!("add_point: no current path in progress, skipping point");
        }
    }

    pub fn finish_path(&mut self) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("finish_path: drawing mode is disabled, skipping path");
            return;
        }

        if let Some(in_progress_path) = self.in_progress_path.take() {
            log::info!("finish_path: finishing path {}", in_progress_path.path_id);
            self.completed_paths.push(in_progress_path);
            self.completed_cache.clear();
        } else {
            log::warn!("finish_path: no path in progress");
        }
    }

    pub fn clear_path(&mut self, path_id: u64) {
        log::info!("clear_path: clearing path {}", path_id);

        // Clear current path if it matches
        if let Some(in_progress) = &self.in_progress_path {
            if in_progress.path_id == path_id {
                self.in_progress_path = None;
            }
        }

        // Remove from completed paths
        self.completed_paths.retain(|path| path.path_id != path_id);
        self.completed_cache.clear();
    }

    pub fn clear(&mut self) {
        self.in_progress_path = None;
        self.completed_paths.clear();
        self.completed_cache.clear();
    }

    /// Returns cached geometry for completed paths.
    pub fn draw_completed(&self, renderer: &Renderer, bounds: Rectangle) -> Geometry {
        self.completed_cache.draw(renderer, bounds.size(), |frame| {
            let glow_stroke = self.make_glow_stroke();
            let core_stroke = self.make_stroke();
            for draw_path in &self.completed_paths {
                if let Some(path) = Self::build_path(&draw_path.points) {
                    // Two-pass rendering: glow first (wider, semi-transparent), then core
                    frame.stroke(&path, glow_stroke);
                    frame.stroke(&path, core_stroke);
                }
            }
        })
    }

    /// Draws in-progress path onto the provided frame.
    pub fn draw_in_progress_to_frame(&self, frame: &mut Frame) {
        if let Some(in_progress) = &self.in_progress_path {
            if let Some(path) = Self::build_path(&in_progress.points) {
                // Two-pass rendering: glow first (wider, semi-transparent), then core
                frame.stroke(&path, self.make_glow_stroke());
                frame.stroke(&path, self.make_stroke());
            }
        }
    }

    fn make_stroke(&self) -> Stroke<'static> {
        Stroke {
            style: stroke::Style::Solid(self.color),
            width: 5.0,
            line_cap: stroke::LineCap::Round,
            line_join: stroke::LineJoin::Round,
            line_dash: stroke::LineDash::default(),
        }
    }

    fn make_glow_stroke(&self) -> Stroke<'static> {
        let mut glow_color = self.color;
        glow_color.a *= 0.60;
        Stroke {
            style: stroke::Style::Solid(glow_color),
            width: 5.0 + 1.5,
            line_cap: stroke::LineCap::Round,
            line_join: stroke::LineJoin::Round,
            line_dash: stroke::LineDash::default(),
        }
    }

    fn build_path(points: &[Position]) -> Option<path::Path> {
        if points.is_empty() {
            return None;
        }

        let mut builder = path::Builder::new();
        builder.move_to(Point::new(points[0].x as f32, points[0].y as f32));
        for point in &points[1..] {
            builder.line_to(Point::new(point.x as f32, point.y as f32));
        }
        Some(builder.build())
    }
}

/// Manager that owns Draw objects mapped by participant sid.
/// Each participant gets their own Draw instance with their assigned color.
#[derive(Default)]
pub struct DrawManager {
    draws: HashMap<String, Draw>,
}

impl DrawManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a new participant with their color.
    pub fn add_participant(&mut self, sid: String, color: &str) {
        log::info!("DrawManager::add_participant: sid={} color={}", sid, color);
        self.draws.insert(sid, Draw::new(color));
    }

    /// Removes a participant and their drawing data.
    pub fn remove_participant(&mut self, sid: &str) {
        log::info!("DrawManager::remove_participant: sid={}", sid);
        self.draws.remove(sid);
    }

    /// Sets the drawing mode for a specific participant.
    pub fn set_drawing_mode(&mut self, sid: &str, mode: DrawingMode) {
        log::debug!("DrawManager::set_drawing_mode: sid={} mode={:?}", sid, mode);
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.set_mode(mode);
        } else {
            log::warn!(
                "DrawManager::set_drawing_mode: participant {} not found",
                sid
            );
        }
    }

    /// Starts a new drawing path for a participant.
    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        log::debug!(
            "DrawManager::draw_start: sid={} point={:?} path_id={}",
            sid,
            point,
            path_id
        );
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.start_path(path_id, point);
        } else {
            log::warn!("DrawManager::draw_start: participant {} not found", sid);
        }
    }

    /// Adds a point to the current drawing path for a participant.
    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        log::debug!("DrawManager::draw_add_point: sid={} point={:?}", sid, point);
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.add_point(point);
        } else {
            log::warn!("DrawManager::draw_add_point: participant {} not found", sid);
        }
    }

    /// Ends the current drawing path for a participant.
    pub fn draw_end(&mut self, sid: &str, point: Position) {
        log::debug!("DrawManager::draw_end: sid={} point={:?}", sid, point);
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.add_point(point);
            draw.finish_path();
        } else {
            log::warn!("DrawManager::draw_end: participant {} not found", sid);
        }
    }

    /// Clears a specific drawing path for a participant.
    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        log::debug!(
            "DrawManager::draw_clear_path: sid={} path_id={}",
            sid,
            path_id
        );
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.clear_path(path_id);
        } else {
            log::warn!(
                "DrawManager::draw_clear_path: participant {} not found",
                sid
            );
        }
    }

    /// Clears all drawing paths for a participant.
    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        log::debug!("DrawManager::draw_clear_all_paths: sid={}", sid);
        if let Some(draw) = self.draws.get_mut(sid) {
            draw.clear();
        } else {
            log::warn!(
                "DrawManager::draw_clear_all_paths: participant {} not found",
                sid
            );
        }
    }

    /// Renders all draws and returns the geometries.
    pub fn draw(&self, renderer: &Renderer, bounds: Rectangle) -> Vec<Geometry> {
        let mut geometries = Vec::with_capacity(self.draws.len() + 1);

        // Collect cached completed geometries from each Draw
        for draw in self.draws.values() {
            geometries.push(draw.draw_completed(renderer, bounds));
        }

        // Draw all in-progress paths into a single frame
        let mut in_progress_frame = Frame::new(renderer, bounds.size());
        for draw in self.draws.values() {
            draw.draw_in_progress_to_frame(&mut in_progress_frame);
        }
        geometries.push(in_progress_frame.into_geometry());

        geometries
    }
}
