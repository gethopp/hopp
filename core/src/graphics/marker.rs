use iced::{widget::canvas, Rectangle, Renderer};

use crate::utils::geometry::Position;

pub struct Marker {
    marker: iced_core::image::Handle,
}

impl Marker {
    pub fn new(texture_path: &String) -> Self {
        let marker =
            iced_core::image::Handle::from_path(format!("{texture_path}/marker_top_left.png"));
        Self { marker }
    }

    pub fn draw(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        translate: &dyn Fn(Position) -> Position,
    ) -> canvas::Geometry {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let top_left = translate(Position { x: 0.0, y: 0.0 });
        let bottom_right = translate(Position { x: 1.0, y: 1.0 });
        let width = 40.0;
        let height = 40.0;

        for (x, y, rotation) in [
            (top_left.x, top_left.y, iced_core::Radians(0.0)),
            (
                top_left.x,
                bottom_right.y - height as f64,
                iced_core::Radians::PI * 1.5,
            ),
            (
                bottom_right.x - width as f64,
                bottom_right.y - height as f64,
                iced_core::Radians::PI,
            ),
            (
                bottom_right.x - width as f64,
                top_left.y,
                iced_core::Radians::PI / 2.0,
            ),
        ] {
            let marker = iced_core::image::Image::new(self.marker.clone()).rotation(rotation);
            frame.draw_image(
                Rectangle {
                    x: x as f32,
                    y: y as f32,
                    width,
                    height,
                },
                marker,
            );
        }

        frame.into_geometry()
    }
}
