//! Overlay window coordinate transformation utilities.
//!
//! This module provides functionality for managing overlay windows and transforming
//! coordinates between different coordinate systems (local window, global screen, percentages).
//! It handles special cases like menubar positioning and display scaling to ensure
//! accurate coordinate mapping for virtual cursors.

use core::fmt;
use std::sync::{Arc, Mutex};

use winit::dpi::PhysicalPosition;

use crate::utils::geometry::{Extent, Frame, Position};

/// Display information used for the overlay window.
pub struct DisplayInfo {
    /* The display's dimensions in pixels. */
    pub display_extent: Extent,
    /* The display's position in global coordinates (pixels). */
    pub display_position: PhysicalPosition<i32>,
    /* The display's scale factor. */
    pub display_scale: f64,
}

/// An overlay window that handles coordinate transformations between different coordinate systems.
///
/// The `OverlayWindow` struct manages the complex coordinate transformations needed when
/// displaying overlay content on top of shared windows or displays. It accounts for:
/// - Menubar positioning and height
/// - Display scaling factors
/// - Converting between pixels, points, and percentages
///
/// It is used for properly showing the virtual cursor in the correct position and
/// translating to global coordinates from display local when simulating mouse events.
pub struct OverlayWindow {
    frame: Option<Arc<Mutex<Frame>>>,
    /* The window's dimensions in pixels. */
    extent: Extent,
    /* The window's position in global coordinates (pixels). */
    position: PhysicalPosition<i32>,
    display_info: DisplayInfo,
    scaled: bool,
}

impl OverlayWindow {
    /// Creates a new `OverlayWindow` with default values.
    ///
    /// All dimensions are set to 0, positions to (0,0), scale to 1.0,
    /// menubar percentage to 0.0, and menubar position to Top.
    ///
    /// # Returns
    ///
    /// A new `OverlayWindow` instance with default values.
    pub fn default() -> Self {
        Self {
            frame: None,
            extent: Extent {
                width: 0.0,
                height: 0.0,
            },
            position: PhysicalPosition::new(0, 0),
            display_info: DisplayInfo {
                display_extent: Extent {
                    width: 0.0,
                    height: 0.0,
                },
                display_position: PhysicalPosition::new(0, 0),
                display_scale: 1.0,
            },
            scaled: false,
        }
    }

    /// Creates a new `OverlayWindow` with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `sharing_window_frame` - The frame of the window/display being shared
    /// * `extent` - The window's dimensions in pixels
    /// * `display_extent` - The display's dimensions in pixels
    /// * `position` - The window's position in global coordinates (pixels)
    /// * `display_position` - The display's position in global coordinates (pixels)
    /// * `display_scale` - The display's scale factor
    /// * `menubar_percentage` - The percentage of screen height occupied by the menubar
    /// * `menubar_position` - Whether the menubar is at the top or bottom
    ///
    /// # Returns
    ///
    /// A new `OverlayWindow` instance with the specified parameters.
    pub fn new(
        frame: Option<Arc<Mutex<Frame>>>,
        extent: Extent,
        position: PhysicalPosition<i32>,
        display_info: DisplayInfo,
        scaled: bool,
    ) -> Self {
        Self {
            frame,
            extent,
            position,
            display_info,
            scaled,
        }
    }

    pub fn source_to_global(&self, position: Position) -> Option<Position> {
        self.source_to_global_with_frame(position, self.capture_frame())
    }

    fn source_to_global_with_frame(
        &self,
        position: Position,
        frame: Option<Frame>,
    ) -> Option<Position> {
        if is_out_of_bounds(position) {
            return None;
        }
        if let Some(frame) = frame {
            if !valid_frame(frame) {
                return None;
            }
            return Some(Position {
                x: frame.origin_x + position.x * frame.extent.width,
                y: frame.origin_y + position.y * frame.extent.height,
            });
        }

        let mut global = Position {
            x: position.x * self.display_info.display_extent.width
                + self.display_info.display_position.x as f64,
            y: position.y * self.display_info.display_extent.height
                + self.display_info.display_position.y as f64,
        };
        if self.scaled {
            global.x /= self.display_info.display_scale;
            global.y /= self.display_info.display_scale;
        }
        Some(global)
    }

    /// Converts global coordinates to local window percentage coordinates.
    ///
    /// This function takes global screen coordinates and converts them to percentage
    /// coordinates relative to the local overlay window. The input coordinates can
    /// be in points or pixels depending on the `scaled` parameter.
    ///
    /// # Arguments
    ///
    /// * `x` - The global x-coordinate
    /// * `y` - The global y-coordinate
    ///
    /// # Returns
    ///
    /// A `Position` struct containing the local percentage coordinates (0.0 to 1.0).
    pub fn local_percentage_from_global(&self, x: f64, y: f64) -> Position {
        let mut scale = 1.0;
        if self.scaled {
            scale = self.display_info.display_scale;
        }
        let x = ((x * scale) - self.position.x as f64) / self.extent.width;
        let y = ((y * scale) - self.position.y as f64) / self.extent.height;

        Position { x, y }
    }

    pub fn global_to_source(&self, position: Position) -> Option<Position> {
        let frame = self.capture_frame();
        if let Some(frame) = frame {
            if !valid_frame(frame) {
                return None;
            }
            let source = Position {
                x: (position.x - frame.origin_x) / frame.extent.width,
                y: (position.y - frame.origin_y) / frame.extent.height,
            };
            return (!is_out_of_bounds(source)).then_some(source);
        }

        let mut scale = 1.0;
        if self.scaled {
            scale = self.display_info.display_scale;
        }
        let source = Position {
            x: ((position.x * scale) - self.display_info.display_position.x as f64)
                / self.display_info.display_extent.width,
            y: ((position.y * scale) - self.display_info.display_position.y as f64)
                / self.display_info.display_extent.height,
        };
        (!is_out_of_bounds(source)).then_some(source)
    }

    /// Converts global screen coordinates to the NSEvent location expected by
    /// `CursorSimulator::post_to_window` on macOS.
    ///
    /// The NSEvent->CGEvent conversion cannot resolve a foreign window number
    /// to an NSWindow, so it treats the location as AppKit screen coordinates
    /// and flips them around the main display's height (`sender_flip_height`,
    /// measured at runtime). The receiving side interprets the delivered event
    /// location as a top-down window-local point, which makes the net transform
    /// `y = sender_flip_height - (position.y - origin_y)` for any display
    /// origin, including negative ones.
    ///
    /// Without a measured `sender_flip_height`, fall back to the previous
    /// heuristic, which is exact only when the window spans the full height of
    /// the main display.
    pub fn global_to_window_local(
        &self,
        position: Position,
        sender_flip_height: Option<f64>,
    ) -> Option<Position> {
        let frame = self.capture_frame()?;
        valid_frame(frame).then_some(Position {
            x: position.x - frame.origin_x,
            y: match sender_flip_height {
                Some(height) => height - (position.y - frame.origin_y),
                None => frame.extent.height - ((position.y - frame.origin_y) - frame.origin_y),
            },
        })
    }

    fn capture_frame(&self) -> Option<Frame> {
        self.frame
            .as_ref()
            .and_then(|frame| frame.lock().ok().map(|frame| *frame))
    }

    pub fn get_display_scale(&self) -> f64 {
        self.display_info.display_scale
    }

    pub fn get_pixel_position(&self, x: f64, y: f64) -> Position {
        Position {
            x: x * self.display_info.display_extent.width / self.display_info.display_scale,
            y: y * self.display_info.display_extent.height / self.display_info.display_scale,
        }
    }

    pub fn get_local_percentage_from_pixel(&self, x: f64, y: f64) -> Position {
        Position {
            x: (x * self.display_info.display_scale) / self.display_info.display_extent.width,
            y: (y * self.display_info.display_scale) / self.display_info.display_extent.height,
        }
    }

    /// Creates a closure that translates source-normalized positions to overlay-local points.
    pub fn create_position_translator(&self) -> impl Fn(Position) -> Position + '_ {
        let frame = self.capture_frame();
        let scale = if self.scaled {
            self.display_info.display_scale
        } else {
            1.0
        };
        move |position: Position| {
            if frame.is_none() {
                return Position {
                    x: position.x * self.display_info.display_extent.width
                        / self.display_info.display_scale,
                    y: position.y * self.display_info.display_extent.height
                        / self.display_info.display_scale,
                };
            }
            let Some(global) = self.source_to_global_with_frame(position, frame) else {
                return Position { x: -1.0, y: -1.0 };
            };
            let local = self.local_percentage_from_global(global.x, global.y);
            Position {
                x: local.x * self.extent.width / scale,
                y: local.y * self.extent.height / scale,
            }
        }
    }
}

fn is_out_of_bounds(position: Position) -> bool {
    !position.x.is_finite()
        || !position.y.is_finite()
        || !(0.0..=1.0).contains(&position.x)
        || !(0.0..=1.0).contains(&position.y)
}

fn valid_frame(frame: Frame) -> bool {
    frame.origin_x.is_finite()
        && frame.origin_y.is_finite()
        && frame.extent.width.is_finite()
        && frame.extent.height.is_finite()
        && frame.extent.width > 0.0
        && frame.extent.height > 0.0
}

impl fmt::Display for OverlayWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "extent: {}, display_extent: {}, position: {:?}, display_position: {:?}, display_scale: {}",
            self.extent,
            self.display_info.display_extent,
            self.position,
            self.display_info.display_position,
            self.display_info.display_scale,
        )
    }
}
