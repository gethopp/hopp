//! Overlay window coordinate transformation utilities.
//!
//! This module provides functionality for managing overlay windows and transforming
//! coordinates between different coordinate systems (local window, global screen, percentages).
//! It handles special cases like menubar positioning and display scaling to ensure
//! accurate coordinate mapping for virtual cursors.

use core::fmt;

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
    /* The frame of the window/display being shared. */
    sharing_window_frame: Frame,
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
            sharing_window_frame: Frame {
                origin_x: 0.,
                origin_y: 0.,
                extent: Extent {
                    width: 0.0,
                    height: 0.0,
                },
            },
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
        sharing_window_frame: Frame,
        extent: Extent,
        position: PhysicalPosition<i32>,
        display_info: DisplayInfo,
        scaled: bool,
    ) -> Self {
        Self {
            sharing_window_frame,
            extent,
            position,
            display_info,
            scaled,
        }
    }

    /// Translates window local percentage coordinates to screen percentage coordinates.
    ///
    /// This function is essential for drawing virtual cursors in the correct position
    /// in the overlay window. It adjusts for menubar positioning and handles the
    /// coordinate system differences between the shared content and the overlay window.
    ///
    /// When sharing a display, the incoming percentage includes the menubar area,
    /// but the overlay window doesn't include it, so this function adjusts the
    /// coordinates accordingly.
    ///
    /// # Arguments
    ///
    /// * `x` - The x-coordinate as a percentage (0.0 to 1.0)
    /// * `y` - The y-coordinate as a percentage (0.0 to 1.0)
    ///
    /// # Returns
    ///
    /// A `Position` struct containing the translated coordinates as percentages.
    pub fn translate_location(&self, x: f64, y: f64) -> Position {
        log::debug!("translate_location: x: {x}, y: {y}");

        if self.sharing_window_frame.extent.width == 0.0
            || self.sharing_window_frame.extent.height == 0.0
        {
            log::debug!("translate_point: client_frame extent is 0.0");
            return Position { x, y };
        }

        /* The following is unused. It will be needed when we support individual window sharing. */
        let width_ratio = self.sharing_window_frame.extent.width / self.extent.width;
        let width_offset = self.sharing_window_frame.origin_x / self.extent.width;
        let x = x * width_ratio + width_offset;

        let height_ratio = (self.sharing_window_frame.extent.height / self.extent.height).min(1.0);
        let height_offset = self.sharing_window_frame.origin_y / self.extent.height;
        let y = y * height_ratio + height_offset;

        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            log::error!("translate_location: x: {x}, y: {y} is out of bounds");
        }

        Position { x, y }
    }

    /// Translates local percentage coordinates to global screen coordinates.
    ///
    /// This function converts coordinates from the local percentage system (0.0 to 1.0)
    /// to global screen coordinates in pixels or points. The `scaled` parameter determines
    /// whether the output should be in points (scaled) or pixels (unscaled).
    ///
    /// Note: This function currently only works when sharing a display, not for
    /// window-local clicks.
    ///
    /// # Arguments
    ///
    /// * `x` - The x-coordinate as a percentage (0.0 to 1.0)
    /// * `y` - The y-coordinate as a percentage (0.0 to 1.0), includes menubar height
    ///
    /// # Returns
    ///
    /// A `Position` struct containing the global coordinates.
    ///
    /// # Platform Notes
    ///
    /// - macOS expects coordinates in points (scaled) for control commands
    /// - Windows expects coordinates in pixels (unscaled)
    pub fn translate_to_global(&self, x: f64, y: f64) -> Position {
        // This doesn't work in window local click, only works when sharing display
        // Here in y the menubar heigh is included.
        let mut x = x * self.display_info.display_extent.width
            + (self.display_info.display_position.x as f64);
        let mut y = y * self.display_info.display_extent.height
            + (self.display_info.display_position.y as f64);
        /*
         * macOS expects the coords in points (scaled) in control commands while windows
         * expects them unscaled.
         * The same applies to every other function that uses the display scale.
         */
        if self.scaled {
            x /= self.display_info.display_scale;
            y /= self.display_info.display_scale;
        }

        Position { x, y }
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

    /// Converts global coordinates to global display percentage coordinates.
    ///
    /// This function takes global screen coordinates and converts them to percentage
    /// coordinates relative to the entire display.
    ///
    /// # Arguments
    ///
    /// * `x` - The global x-coordinate
    /// * `y` - The global y-coordinate
    ///
    /// # Returns
    ///
    /// A `Position` struct containing the global display percentage coordinates (0.0 to 1.0).
    ///
    /// # Note
    ///
    /// Similar to `local_percentage_from_global`, this function handles the conversion
    /// between points and pixels using the display scale factor.
    pub fn global_percentage_from_global(&self, x: f64, y: f64) -> Position {
        let mut scale = 1.0;
        if self.scaled {
            scale = self.display_info.display_scale;
        }
        let x = ((x * scale) - (self.display_info.display_position.x as f64))
            / self.display_info.display_extent.width;
        let y = ((y * scale) - (self.display_info.display_position.y as f64))
            / self.display_info.display_extent.height;

        Position { x, y }
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
}

impl fmt::Display for OverlayWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sharing_window_frame: {}, extent: {}, display_extent: {}, position: {:?}, display_position: {:?}, display_scale: {}",
            self.sharing_window_frame,
            self.extent,
            self.display_info.display_extent,
            self.position,
            self.display_info.display_position,
            self.display_info.display_scale,
        )
    }
}
