use core::fmt;
use std::cmp::max;

use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize)]
pub struct Extent {
    pub width: f64,
    pub height: f64,
}

impl fmt::Display for Extent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "width: {}, height: {}", self.width, self.height)
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Frame {
    pub origin_x: f64,
    pub origin_y: f64,
    pub extent: Extent,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            origin_x: 0.,
            origin_y: 0.,
            extent: Extent {
                width: 0.,
                height: 0.,
            },
        }
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "origin_x: {}, origin_y: {}, extent: {}",
            self.origin_x, self.origin_y, self.extent
        )
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0., y: 0. }
    }
}

pub fn aspect_fit(width: u32, height: u32, target_width: u32, target_height: u32) -> (u32, u32) {
    let size = max(target_width, target_height);
    let (fitted_width, fitted_height) = if width >= height {
        let aspect_ratio = height as f32 / width as f32;
        (size, ((size as f32) * aspect_ratio) as u32)
    } else {
        let aspect_ratio = width as f32 / height as f32;
        (((size as f32) / aspect_ratio) as u32, size)
    };

    if fitted_width > width || fitted_height > height {
        (width, height)
    } else {
        (fitted_width, fitted_height)
    }
}
