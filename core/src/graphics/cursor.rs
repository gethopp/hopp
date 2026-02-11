//! Image-based cursor for iced canvas rendering.
//!
//! This module provides a `Cursor` that renders user cursors as pre-rendered
//! PNG images on an iced canvas frame, using `svg_renderer::render_user_badge_to_png`
//! to convert SVGs to PNGs at construction time.

use crate::utils::geometry::Position;
use crate::utils::svg_renderer::{render_user_badge_to_png, SvgRenderError};
use iced::widget::canvas::Frame;
use iced::Rectangle;

/// Cursor display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMode {
    /// Normal arrow cursor
    Normal,
    /// Pointer/hand cursor
    Pointer,
}

/// An image-based cursor for rendering on iced canvas frames.
#[derive(Debug)]
pub struct Cursor {
    /// Visible name displayed on the cursor
    visible_name: String,
    /// (handle, (width, height)) for normal arrow cursor
    normal_cursor: (iced_core::image::Handle, (f32, f32)),
    /// (handle, (width, height)) for pointer/hand cursor
    pointer_cursor: (iced_core::image::Handle, (f32, f32)),
    /// Current position of the cursor
    position: Option<Position>,
    /// Current cursor display mode
    mode: CursorMode,
}

impl Cursor {
    /// Creates a new `Cursor` with the given color and name.
    pub fn new(color: &str, name: &str) -> Result<Self, SvgRenderError> {
        let normal_png = render_user_badge_to_png(color, name, false)?;
        let pointer_png = render_user_badge_to_png(color, name, true)?;

        let normal_dims = image::load_from_memory(&normal_png).map_err(|e| {
            SvgRenderError::PngSaveError(format!("Failed to read PNG dimensions: {e}"))
        })?;
        let pointer_dims = image::load_from_memory(&pointer_png).map_err(|e| {
            SvgRenderError::PngSaveError(format!("Failed to read PNG dimensions: {e}"))
        })?;

        let normal_cursor = (
            iced_core::image::Handle::from_bytes(normal_png),
            (
                normal_dims.width() as f32 / 2.5,
                normal_dims.height() as f32 / 2.5,
            ),
        );
        let pointer_cursor = (
            iced_core::image::Handle::from_bytes(pointer_png),
            (
                pointer_dims.width() as f32 / 2.5,
                pointer_dims.height() as f32 / 2.5,
            ),
        );

        Ok(Self {
            visible_name: name.to_string(),
            normal_cursor,
            pointer_cursor,
            position: None,
            mode: CursorMode::Normal,
        })
    }

    /// Returns the visible name displayed on this cursor.
    pub fn visible_name(&self) -> &str {
        &self.visible_name
    }

    /// Sets the cursor display mode.
    pub fn set_mode(&mut self, mode: CursorMode) {
        self.mode = mode;
    }

    /// Sets the cursor position.
    pub fn set_position(&mut self, position: Option<Position>) {
        self.position = position;
    }

    /// Draws the cursor onto an iced canvas frame.
    pub fn draw(&self, frame: &mut Frame) {
        if self.position.is_none() {
            return;
        }

        let (handle, (width, height)) = match self.mode {
            CursorMode::Pointer => &self.pointer_cursor,
            CursorMode::Normal => &self.normal_cursor,
        };

        let image = iced_core::image::Image::new(handle.clone());
        let position = self.position.as_ref().unwrap();
        frame.draw_image(
            Rectangle {
                x: position.x as f32,
                y: position.y as f32,
                width: *width,
                height: *height,
            },
            image,
        );
    }
}
