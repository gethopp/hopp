use iced::widget::canvas::{self, stroke, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size};

use crate::utils::geometry::Position;

pub struct Marker;

impl Marker {
    pub fn new() -> Self {
        Self
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
        const BORDER_WIDTH: f32 = 4.0;
        let inset = BORDER_WIDTH / 2.0;
        let border = Path::rectangle(
            Point::new(top_left.x as f32 + inset, top_left.y as f32 + inset),
            Size::new(
                ((bottom_right.x - top_left.x) as f32 - BORDER_WIDTH).max(0.0),
                ((bottom_right.y - top_left.y) as f32 - BORDER_WIDTH).max(0.0),
            ),
        );
        frame.stroke(
            &border,
            Stroke {
                style: stroke::Style::Solid(Color::from_rgba(0.28, 0.12, 0.58, 0.98)),
                width: BORDER_WIDTH,
                ..Stroke::default()
            },
        );

        frame.into_geometry()
    }
}
