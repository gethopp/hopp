use iced::{widget::canvas, Rectangle, Renderer};

pub struct Marker {
    marker: iced_core::image::Handle,
    cache: canvas::Cache,
}

impl Marker {
    pub fn new(texture_path: &String) -> Self {
        let cache = canvas::Cache::new();
        let marker =
            iced_core::image::Handle::from_path(format!("{texture_path}/marker_top_left.png"));
        Self { marker, cache }
    }

    pub fn draw(&self, renderer: &Renderer, bounds: Rectangle) -> canvas::Geometry {
        self.cache.draw(renderer, bounds.size(), |frame| {
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
        })
    }
}
