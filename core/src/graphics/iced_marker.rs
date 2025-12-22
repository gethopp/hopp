use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use iced::widget::canvas;
use iced::{mouse, Alignment, Color, Length, Radians, Rectangle, Rotation, Theme};
use iced_wgpu::core::Element;
use image::GenericImageView;

/// Represents the four possible positions where markers can be placed.
///
/// Markers are positioned at the corners of the window/capture area to provide
/// visual feedback about the boundaries.
#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum MarkerPosition {
    /// Top-left corner of the window
    TopLeft,
    /// Top-right corner of the window
    TopRight,
    /// Bottom-left corner of the window
    BottomLeft,
    /// Bottom-right corner of the window
    BottomRight,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Message {}

pub struct Marker {
    marker: iced_core::image::Handle,
    canvas_cache: canvas::Cache,
}

pub struct OverlaySurface<'a> {
    cache: &'a canvas::Cache,
    marker: iced_core::image::Handle,
}

impl<'a> std::fmt::Debug for OverlaySurface<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OverlaySurface")
    }
}

impl<'a> OverlaySurface<'a> {
    pub fn new(cache: &'a canvas::Cache, marker: iced_core::image::Handle) -> Self {
        Self { cache, marker }
    }
}

impl<'a, Message> canvas::Program<Message> for OverlaySurface<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // We prepare a new `Frame`
        let markers = self.cache.draw(renderer, bounds.size(), |frame| {
            // TODO: check if this can be improved
            let image_handle = iced_core::image::Image::new(self.marker.clone());
            let width = 40.0;
            let height = 40.0;
            frame.draw_image(
                Rectangle {
                    x: 0.,
                    y: 0.,
                    width: width,
                    height: height,
                },
                image_handle,
            );
            let image_handle = iced_core::image::Image::new(self.marker.clone());
            let image_handle = image_handle.rotation(iced_core::Radians::PI * 1.5);
            frame.draw_image(
                Rectangle {
                    x: 0.,
                    y: bounds.height - height,
                    width: width,
                    height: height,
                },
                image_handle,
            );
            let image_handle = iced_core::image::Image::new(self.marker.clone());
            let image_handle = image_handle.rotation(iced_core::Radians::PI);
            frame.draw_image(
                Rectangle {
                    x: bounds.width - width,
                    y: bounds.height - height,
                    width: width,
                    height: height,
                },
                image_handle,
            );
            let image_handle = iced_core::image::Image::new(self.marker.clone());
            let image_handle = image_handle.rotation(iced_core::Radians::PI / 2.0);
            frame.draw_image(
                Rectangle {
                    x: bounds.width - width,
                    y: 0.,
                    width: width,
                    height: height,
                },
                image_handle,
            );
        });
        vec![markers]
    }
}

impl Marker {
    pub fn new(texture_path: &String) -> Self {
        // Define marker image files and their positions
        let image_path = format!("{texture_path}/marker_top_left.png");
        let marker = iced_core::image::Handle::from_path(image_path);
        Self {
            marker,
            canvas_cache: canvas::Cache::new(),
        }
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        canvas(OverlaySurface::new(&self.canvas_cache, self.marker.clone()))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
