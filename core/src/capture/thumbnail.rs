//! Share-picker thumbnails.

#[cfg(target_os = "macos")]
#[path = "thumbnail_macos.rs"]
mod platform;

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Thumbnail;

    pub fn window_thumbnail(_window_id: u32) -> Option<Thumbnail> {
        None
    }

    pub fn display_thumbnail(_display_id: u32) -> Option<Thumbnail> {
        None
    }
}

use std::sync::Arc;

/// RGBA thumbnail with a reusable iced image handle.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    pub handle: iced_core::image::Handle,
}

impl Thumbnail {
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        let rgba: Arc<[u8]> = rgba.into();
        let handle =
            iced_core::image::Handle::from_rgba(width, height, rgba.as_ref().to_vec());
        Self {
            width,
            height,
            rgba,
            handle,
        }
    }

    pub fn image_handle(&self) -> iced_core::image::Handle {
        self.handle.clone()
    }
}

pub use platform::{display_thumbnail, window_thumbnail};
