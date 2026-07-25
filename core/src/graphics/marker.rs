use iced::widget::canvas::{self, path, stroke, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer};

pub struct Marker {
    cache: canvas::Cache,
}

impl Marker {
    pub fn new(_texture_path: &String) -> Self {
        Self {
            cache: canvas::Cache::new(),
        }
    }

    /// Draws corner marks around `content` when provided, otherwise around `bounds`.
    pub fn draw(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        content: Option<Rectangle>,
    ) -> canvas::Geometry {
        // Content rect changes without bounds size changing (window vs display share),
        // so clear the cache to avoid stale corner positions.
        self.cache.clear();
        let content = content
            .filter(|rect| rect.width > 0.0 && rect.height > 0.0)
            .unwrap_or(bounds);

        self.cache.draw(renderer, bounds.size(), |frame| {
            let arm = 28.0_f32;
            let radius = 8.0_f32;
            let stroke = Stroke {
                style: stroke::Style::Solid(Color::from_rgb(1.0, 0.0, 1.0)),
                width: 3.0,
                line_cap: stroke::LineCap::Round,
                line_join: stroke::LineJoin::Round,
                line_dash: stroke::LineDash::default(),
            };

            let left = content.x;
            let top = content.y;
            let right = content.x + content.width;
            let bottom = content.y + content.height;

            frame.stroke(&rounded_corner(left, top, arm, radius, Corner::TopLeft), stroke);
            frame.stroke(
                &rounded_corner(right, top, arm, radius, Corner::TopRight),
                stroke,
            );
            frame.stroke(
                &rounded_corner(left, bottom, arm, radius, Corner::BottomLeft),
                stroke,
            );
            frame.stroke(
                &rounded_corner(right, bottom, arm, radius, Corner::BottomRight),
                stroke,
            );
        })
    }
}

#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

fn rounded_corner(x: f32, y: f32, arm: f32, radius: f32, corner: Corner) -> Path {
    let radius = radius.min(arm * 0.45);
    let mut builder = path::Builder::new();

    match corner {
        Corner::TopLeft => {
            builder.move_to(Point::new(x, y + arm));
            builder.line_to(Point::new(x, y + radius));
            builder.quadratic_curve_to(Point::new(x, y), Point::new(x + radius, y));
            builder.line_to(Point::new(x + arm, y));
        }
        Corner::TopRight => {
            builder.move_to(Point::new(x - arm, y));
            builder.line_to(Point::new(x - radius, y));
            builder.quadratic_curve_to(Point::new(x, y), Point::new(x, y + radius));
            builder.line_to(Point::new(x, y + arm));
        }
        Corner::BottomLeft => {
            builder.move_to(Point::new(x, y - arm));
            builder.line_to(Point::new(x, y - radius));
            builder.quadratic_curve_to(Point::new(x, y), Point::new(x + radius, y));
            builder.line_to(Point::new(x + arm, y));
        }
        Corner::BottomRight => {
            builder.move_to(Point::new(x - arm, y));
            builder.line_to(Point::new(x - radius, y));
            builder.quadratic_curve_to(Point::new(x, y), Point::new(x, y - radius));
            builder.line_to(Point::new(x, y - arm));
        }
    }

    builder.build()
}
