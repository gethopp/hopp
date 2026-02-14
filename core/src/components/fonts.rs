//! Shared Geist font constants for iced rendering.
//!
//! Font *descriptors* (zero-cost references to the loaded font family) and
//! compile-time-embedded font *bytes*.  Every window that creates an iced
//! renderer should call `load_fonts` once to register the family with the
//! global font system.

use std::borrow::Cow;

use iced::Font;

/// Geist Regular (weight 400).
pub const GEIST_REGULAR: Font = Font::with_name("Geist");

/// Geist Medium (weight 500).
pub const GEIST_MEDIUM: Font = Font {
    family: iced::font::Family::Name("Geist"),
    weight: iced::font::Weight::Medium,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

pub const GEIST_REGULAR_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/geist/Geist-Regular.otf");
pub const GEIST_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/geist/Geist-Medium.otf");

/// Register Geist font data with the global iced font system.
///
/// Call this once per renderer (idempotent — loading the same bytes twice is harmless).
pub fn load_fonts() {
    let mut font_system = iced_wgpu::graphics::text::font_system()
        .write()
        .expect("Failed to lock font system");
    font_system.load_font(Cow::Borrowed(GEIST_REGULAR_BYTES));
    font_system.load_font(Cow::Borrowed(GEIST_MEDIUM_BYTES));
}
