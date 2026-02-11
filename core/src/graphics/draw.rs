use std::time::{Duration, Instant};

use iced::widget::canvas::{path, stroke, Cache, Frame, Geometry, Stroke};
use iced::{Color, Point, Rectangle, Renderer};

use crate::{room_service::DrawingMode, utils::geometry::Position};

const PATH_EXPIRY_DURATION: Duration = Duration::from_secs(3);

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
    finished_at: Option<Instant>,
}

impl DrawPath {
    pub fn new(path_id: u64, point: Position) -> Self {
        Self {
            path_id,
            points: vec![point],
            finished_at: None,
        }
    }
}

pub struct Draw {
    in_progress_path: Option<DrawPath>,
    completed_paths: Vec<DrawPath>,
    completed_cache: Cache,
    mode: DrawingMode,
    color: Color,
    auto_clear: bool,
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
    pub fn new(color: &str, auto_clear: bool) -> Self {
        Self {
            in_progress_path: None,
            completed_paths: Vec::new(),
            completed_cache: Cache::new(),
            mode: DrawingMode::Disabled,
            color: color_from_hex(color),
            auto_clear,
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

        if let Some(mut in_progress_path) = self.in_progress_path.take() {
            log::info!("finish_path: finishing path {}", in_progress_path.path_id);
            in_progress_path.finished_at = Some(Instant::now());
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

    pub fn clear_expired_paths(&mut self) -> Vec<u64> {
        if !self.auto_clear {
            return Vec::new();
        }

        // Only clear in non-permanent mode
        if let DrawingMode::Draw(settings) = &self.mode {
            if settings.permanent {
                return Vec::new();
            }
        } else {
            return Vec::new();
        }

        let now = Instant::now();
        let mut removed_ids = Vec::new();

        self.completed_paths.retain(|path| {
            if let Some(finished_at) = path.finished_at {
                let should_keep = now.duration_since(finished_at) < PATH_EXPIRY_DURATION;
                if !should_keep {
                    removed_ids.push(path.path_id);
                }
                should_keep
            } else {
                true
            }
        });

        if !removed_ids.is_empty() {
            self.completed_cache.clear();
        }

        removed_ids
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
