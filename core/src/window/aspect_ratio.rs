use crate::utils::geometry::Extent;

#[cfg_attr(target_os = "macos", path = "aspect_ratio/macos.rs")]
#[cfg_attr(target_os = "windows", path = "aspect_ratio/windows.rs")]
mod platform;

pub use platform::AspectRatioEnforcer;

pub(crate) struct WindowConstant;

impl WindowConstant {
    pub const DEFAULT_WIDTH: f64 = 600.0;
    pub const MIN_WIDTH: f64 = 500.0;
    pub const PADDING: f32 = 12.0;
    pub const HEADER_HEIGHT: f32 = 42.0;
    pub const HEADER_SIDE_PADDING: f32 = 4.0;
    pub const SKELETON_H: f64 = Self::HEADER_HEIGHT as f64 + Self::PADDING as f64;
    pub const SKELETON_W: f64 = 2.0 * Self::PADDING as f64;
}

pub fn min_window_size_for_aspect(aspect: f64) -> (f64, f64) {
    let min_content_w = WindowConstant::MIN_WIDTH - WindowConstant::SKELETON_W;
    let min_content_h = min_content_w / aspect;
    (
        WindowConstant::MIN_WIDTH,
        min_content_h + WindowConstant::SKELETON_H,
    )
}

pub const fn min_window_size() -> (f64, f64) {
    let min_content_w = WindowConstant::MIN_WIDTH - WindowConstant::SKELETON_W;
    let min_content_h = min_content_w / (16.0 / 9.0);
    (
        WindowConstant::MIN_WIDTH,
        min_content_h + WindowConstant::SKELETON_H,
    )
}

pub const fn default_window_size() -> (f64, f64) {
    let content_w = WindowConstant::DEFAULT_WIDTH - WindowConstant::SKELETON_W;
    let content_h = content_w / (16.0 / 9.0);
    (
        WindowConstant::DEFAULT_WIDTH,
        content_h + WindowConstant::SKELETON_H,
    )
}

pub fn calculate_max_window_size(available: Extent, stream_aspect: f64) -> Option<(f64, f64)> {
    if available.width <= 0.0 || available.height <= 0.0 {
        log::warn!("calculate_max_window_size: available space is zero or negative, falling back");
        return None;
    }

    let max_content_h = available.height - WindowConstant::SKELETON_H;
    let max_content_w = available.width - WindowConstant::SKELETON_W;

    let (content_w, content_h) = if max_content_w / max_content_h > stream_aspect {
        (max_content_h * stream_aspect, max_content_h)
    } else {
        (max_content_w, max_content_w / stream_aspect)
    };

    Some((
        content_w + WindowConstant::SKELETON_W,
        content_h + WindowConstant::SKELETON_H,
    ))
}
