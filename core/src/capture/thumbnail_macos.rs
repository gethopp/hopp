//! Capture small RGBA thumbnails for the share picker (macOS).

use core_graphics::display::{CGDisplay, CGPoint, CGRect, CGSize};
use core_graphics::image::CGImage;
use core_graphics::window::{
    create_image, kCGWindowImageBoundsIgnoreFraming, kCGWindowImageNominalResolution,
    kCGWindowListOptionIncludingWindow,
};
use image::imageops::{resize, FilterType};
use image::RgbaImage;

use super::Thumbnail;

const MAX_THUMB_WIDTH: u32 = 320;
const MAX_THUMB_HEIGHT: u32 = 180;

pub fn window_thumbnail(window_id: u32) -> Option<Thumbnail> {
    // CGRectNull → capture the window's own bounds.
    let bounds = CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(0., 0.));
    let image = create_image(
        bounds,
        kCGWindowListOptionIncludingWindow,
        window_id,
        kCGWindowImageBoundsIgnoreFraming | kCGWindowImageNominalResolution,
    )?;
    cgimage_to_thumbnail(&image)
}

pub fn display_thumbnail(display_id: u32) -> Option<Thumbnail> {
    let image = CGDisplay::new(display_id).image()?;
    cgimage_to_thumbnail(&image)
}

fn cgimage_to_thumbnail(image: &CGImage) -> Option<Thumbnail> {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }

    let stride = image.bytes_per_row();
    let bpp = image.bits_per_pixel() / 8;
    if bpp < 4 {
        return None;
    }

    let data = image.data();
    let bytes = data.bytes();
    if bytes.len() < stride * height {
        return None;
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = &bytes[y * stride..y * stride + width * bpp];
        for px in row.chunks_exact(bpp) {
            // CGWindow / CGDisplay images are typically BGRA.
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    let src = RgbaImage::from_raw(width as u32, height as u32, rgba)?;
    let (tw, th) = fit_size(width as u32, height as u32, MAX_THUMB_WIDTH, MAX_THUMB_HEIGHT);
    let resized = if tw == width as u32 && th == height as u32 {
        src
    } else {
        resize(&src, tw, th, FilterType::Triangle)
    };

    Some(Thumbnail::from_rgba(
        resized.width(),
        resized.height(),
        resized.into_raw(),
    ))
}

fn fit_size(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }
    let scale = (max_w as f32 / width as f32)
        .min(max_h as f32 / height as f32)
        .min(1.0);
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}
