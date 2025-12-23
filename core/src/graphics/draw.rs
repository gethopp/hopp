use std::time::Instant;

use crate::{room_service::DrawingMode, utils::geometry::Position};

const PATH_EXPIRATION_TIME: std::time::Duration = std::time::Duration::from_millis(5000);

#[derive(Debug, Clone)]
struct DrawPath {
    points: Vec<Position>,
    finished_at: Option<Instant>,
}

impl DrawPath {
    pub fn new(point: Position) -> Self {
        Self {
            points: vec![point],
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Draw {
    in_progress_path: Option<DrawPath>,
    completed_paths: Vec<DrawPath>,
    mode: DrawingMode,
}

impl Draw {
    pub fn new() -> Self {
        Self {
            in_progress_path: None,
            completed_paths: Vec::new(),
            mode: DrawingMode::Disabled,
        }
    }

    pub fn set_mode(&mut self, mode: DrawingMode) {
        self.mode = mode;
    }

    pub fn add_point(&mut self, point: Position) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("add_point: drawing mode is disabled, skipping point");
            return;
        }

        if let Some(in_progress_path) = self.in_progress_path.as_mut() {
            in_progress_path.points.push(point);
        } else {
            self.in_progress_path = Some(DrawPath::new(point));
        }
    }

    pub fn finish_path(&mut self) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("finish_path: drawing mode is disabled, skipping path");
            return;
        }

        if let Some(mut in_progress_path) = self.in_progress_path.take() {
            match self.mode {
                DrawingMode::Draw(settings) => {
                    if !settings.permanent {
                        in_progress_path.finished_at = Some(Instant::now());
                    }
                }
                _ => {}
            }
            self.completed_paths.push(in_progress_path);
        }
    }

    pub fn clear(&mut self) {
        self.in_progress_path = None;
        self.completed_paths.clear();
    }

    pub fn update_completed_paths(&mut self) {
        if self.mode == DrawingMode::Disabled {
            log::warn!("update_completed_paths: drawing mode is disabled, skipping paths");
            return;
        }

        self.completed_paths.retain(|path| {
            if path.finished_at.is_none() {
                return true;
            }
            let finished_at = path.finished_at.as_ref().unwrap();
            if finished_at.elapsed() < PATH_EXPIRATION_TIME {
                true
            } else {
                false
            }
        });
    }
}
