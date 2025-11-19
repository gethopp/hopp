//! # User Badge SVG Renderer
//!
//! This module provides functionality to render user badges to PNG format.
//! It uses the `resvg` crate for high-quality SVG rendering with a predefined template.
//!

use fontdb::Database;
use resvg::{tiny_skia, usvg};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SvgRenderError {
    #[error("Failed to parse SVG: {0}")]
    SvgParseError(String),
    #[error("Failed to create pixmap")]
    PixmapCreationError,
    #[error("Failed to save PNG: {0}")]
    PngSaveError(String),
}

/// Calculate dynamic box width based on text length
/// Increases box width for longer text to ensure it fits comfortably
fn calculate_box_width(text: &str) -> f32 {
    let base_width = 29.0;
    let base_chars = 2;
    let char_width = 6.5;

    if text.len() <= base_chars {
        base_width
    } else {
        base_width + ((text.len() - base_chars) as f32 * char_width)
    }
}

fn get_box_width(text: &str, fontdb: std::sync::Arc<Database>) -> Result<f32, SvgRenderError> {
    // Create a minimal SVG just for text measurement
    let measurement_svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg">
            <text font-family="Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
                  font-size="11.606"
                  font-weight="600"
                  letter-spacing="0.05em">{text}</text>
        </svg>"#
    );

    let usvg_options = usvg::Options {
        fontdb,
        ..Default::default()
    };

    let padding = 13.0;
    match usvg::Tree::from_str(&measurement_svg, &usvg_options) {
        Ok(tree) => {
            // Use the tree's bounding box instead of searching for text nodes
            let bbox = tree.root().abs_bounding_box();
            Ok(bbox.width() + padding)
        }
        Err(_) => {
            // Fallback to improved estimation
            Err(SvgRenderError::SvgParseError(
                "Failed to parse SVG".to_string(),
            ))
        }
    }
}

/// Renders a user avatar badge to PNG data using a predefined SVG template
///
/// This function uses a specific SVG template that creates a speech bubble design
/// with customizable color and name text.
///
/// # Arguments
///
/// * `color` - Hex color code (e.g., "#FF5733" or "red") for the badge background
/// * `name` - Name text to display in the badge
///
/// # Returns
///
/// Returns `Ok(Vec<u8>)` containing PNG data on success or `Err(SvgRenderError)` on failure
pub fn render_user_badge_to_png(
    color: &str,
    name: &str,
    pointer: bool,
) -> Result<Vec<u8>, SvgRenderError> {
    // Create font database
    let mut fontdb = Database::new();
    fontdb.load_system_fonts();
    let fontdb = std::sync::Arc::new(fontdb);

    let mut box_width = if let Ok(width) = get_box_width(name, fontdb.clone()) {
        width
    } else {
        log::error!("Failed to get box width for name: {name} using fallback");
        calculate_box_width(name)
    };

    let mut name = name.to_string();
    /* This might not work perfectly for every name. */
    if box_width > 152.0 {
        box_width = 152.0;
        name = name.chars().take(17).collect::<String>() + "...";
    };

    // Calculate text x position with left padding
    // For pointer template: rect starts at x=25, original text at x=31 (6px padding)
    // For regular template: rect starts at x=27.3704, original text at x=33.4 (6.0296px padding)
    let text_x_pointer = 25.0 + 6.0;
    let text_x_regular = 27.3704 + 6.0296;

    // Calculate dynamic SVG dimensions based on box_width
    // Original: box_width=70, viewBox width=112 (pointer) or 114 (regular)
    // The difference accounts for the rect x position + padding on the right
    let svg_width_pointer = box_width + 42.0; // 25 (left margin) + 70 (original box) + 17 (right margin) = 112
    let svg_width_regular = box_width + 44.0; // 27.3704 (left margin) + 70 (original box) + 16.6296 (right margin) ≈ 114

    // Choose SVG template based on pointer flag
    let svg_template = if pointer {
        // Pointer template with hand cursor
        format!(
            r#"<svg width="{svg_width}" height="83" viewBox="0 0 {svg_width} 83" fill="none" xmlns="http://www.w3.org/2000/svg">
<g filter="url(#filter0_di_3991_4626)">
<path d="M6.27136 21.4178L6.67935 23.8862L8.12589 25.035L9.76073 25.6614L18.5464 34.4471L29.7005 30.483L29.9283 26.8257L29.8092 22.2086L26.9229 12.8857L24.4731 11.7369L21.9631 11.5066L16.2023 11.7369L13.7304 12.0362L10.0361 6.10558L7.29421 7.09688L12.1631 21.9036L8.42991 20.153L6.27136 21.4178Z" fill="{color}"/>
</g>
<path d="M5.97937 7.81435L7.3503 7.3187L12.8025 22.3989L11.4315 22.8946L10.9359 21.5236L9.56495 22.0193L9.0693 20.6484L10.4402 20.1527L5.97937 7.81435ZM26.3957 12.834L29.8653 22.4305L31.2362 21.9348L27.7667 12.3383L26.3957 12.834ZM4.46086 20.7644L5.94782 24.8772L7.31875 24.3815L6.32744 21.6397L9.0693 20.6484L8.57365 19.2774L4.46086 20.7644ZM11.5476 27.503L10.5563 24.7611L9.18533 25.2568L10.1766 27.9987L11.5476 27.503ZM13.9098 29.7492L12.9185 27.0074L11.5476 27.503L12.5389 30.2449L13.9098 29.7492ZM16.272 31.9954L17.759 36.1082L31.4683 31.1517L29.9844 27.0475L28.6135 27.5431L29.6017 30.2764L18.6343 34.2416L17.6429 31.4998L16.272 31.9954L15.2807 29.2536L13.9098 29.7492L14.9011 32.4911L16.272 31.9954ZM29.9813 27.0389L31.3522 26.5432L29.8653 22.4305L28.4944 22.9261L29.9813 27.0389ZM9.18533 25.2568L8.68967 23.8859L7.31875 24.3815L7.8144 25.7524L9.18533 25.2568ZM24.5291 11.9587L25.0248 13.3296L26.3957 12.834L25.9001 11.463L24.5291 11.9587ZM21.2916 11.5791L23.2742 17.0628L24.6452 16.5671L23.1582 12.4543L24.5291 11.9587L24.0335 10.5878L21.2916 11.5791ZM16.6832 11.6951L18.6658 17.1788L20.0367 16.6831L18.5498 12.5704L21.2916 11.5791L20.796 10.2081L16.6832 11.6951ZM13.4457 11.3155L11.4631 5.83174L10.0922 6.32739L14.553 18.6658L15.9239 18.1701L13.9413 12.6864L16.6832 11.6951L16.1875 10.3242L13.4457 11.3155ZM6.85464 5.94777L7.3503 7.3187L10.0922 6.32739L9.5965 4.95647L6.85464 5.94777Z" fill="white"/>
<g filter="url(#filter1_di_3991_4626)">
<rect x="25" y="30.5864" width="{box_width}" height="35" rx="17.5" fill="{color}" shape-rendering="crispEdges"/>
<rect x="25.5503" y="31.1367" width="{box_width_stroke}" height="33.8994" rx="16.9497" stroke="white" stroke-opacity="0.7" stroke-width="1.10065" shape-rendering="crispEdges"/>
<text fill="white" xml:space="preserve" style="white-space: pre" font-family="Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif" font-size="11.606" font-weight="600" letter-spacing="0.05em"><tspan x="{text_x}" y="52.3264">{name}</tspan></text>
</g>
<defs>
<filter id="filter0_di_3991_4626" x="2.9693" y="3.90417" width="30.2609" height="34.9454" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="1.10065"/>
<feGaussianBlur stdDeviation="1.65097"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.35 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3991_4626"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3991_4626" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="2.37363" dy="3.16484"/>
<feGaussianBlur stdDeviation="3.95604"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.4 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow_3991_4626"/>
</filter>
<filter id="filter1_di_3991_4626" x="8.49028" y="14.0767" width="{filter_width}" height="68.0194" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset/>
<feGaussianBlur stdDeviation="8.25486"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.1 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3991_4626"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3991_4626" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="3.16484"/>
<feGaussianBlur stdDeviation="4.35165"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.75 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow_3991_4626"/>
</filter>
</defs>
</svg>"#,
            color = color,
            name = name,
            box_width = box_width,
            box_width_stroke = box_width - 1.10065,
            filter_width = box_width + 16.5097 * 2.0,
            text_x = text_x_pointer,
            svg_width = svg_width_pointer,
        )
    } else {
        // Regular cursor template
        format!(
            r#"<svg width="{svg_width}" height="87" viewBox="0 0 {svg_width} 87" fill="none" xmlns="http://www.w3.org/2000/svg">
<g filter="url(#filter0_di_3982_4518)">
<path d="M14.3677 38.986C13.2653 40.5665 10.8079 40.068 10.4086 38.1828L3.35233 4.86443C2.96427 3.0321 4.89624 1.58308 6.54664 2.46863L37.3879 19.017C39.1564 19.9659 38.8364 22.5929 36.8918 23.0895L23.7692 26.4409C23.2578 26.5715 22.8103 26.8815 22.5084 27.3144L14.3677 38.986Z" fill="{color}"/>
<path d="M3.89087 4.75024C3.59995 3.3761 5.04864 2.28938 6.28638 2.95337L37.1282 19.5022C38.4539 20.214 38.2139 22.1831 36.7561 22.5559L23.6331 25.9075C22.9939 26.0707 22.4343 26.4582 22.0569 26.9993L13.9163 38.6711C13.0895 39.8565 11.247 39.4825 10.9475 38.0686L3.89087 4.75024Z" stroke="white" stroke-opacity="0.7" stroke-width="1.10065"/>
</g>
<g filter="url(#filter1_di_3982_4518)">
<rect x="27.3704" y="35.1531" width="{box_width}" height="35" rx="17.5" fill="{color}" shape-rendering="crispEdges"/>
<rect x="27.9207" y="35.7034" width="{box_width_stroke}" height="33.8994" rx="16.9497" stroke="white" stroke-opacity="0.7" stroke-width="1.10065" shape-rendering="crispEdges"/>
<text fill="white" xml:space="preserve" style="white-space: pre" font-family="Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif" font-size="11.606" font-weight="600" letter-spacing="0.05em"><tspan x="{text_x}" y="56.8931">{name}</tspan></text>
</g>
<defs>
<filter id="filter0_di_3982_4518" x="5.8651e-05" y="0.00012064" width="41.851" height="44.3319" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="1.10065"/>
<feGaussianBlur stdDeviation="1.65097"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.35 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3982_4518"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3982_4518" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="2.37363" dy="3.16484"/>
<feGaussianBlur stdDeviation="3.95604"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.4 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow_3982_4518"/>
</filter>
<filter id="filter1_di_3982_4518" x="10.8606" y="18.6434" width="{filter_width}" height="68.0194" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset/>
<feGaussianBlur stdDeviation="8.25486"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.1 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3982_4518"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3982_4518" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="3.16484"/>
<feGaussianBlur stdDeviation="4.35165"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.75 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow_3982_4518"/>
</filter>
</defs>
</svg>"#,
            color = color,
            name = name,
            box_width = box_width,
            box_width_stroke = box_width - 1.10065,
            filter_width = box_width + 16.5097 * 2.0,
            text_x = text_x_regular,
            svg_width = svg_width_regular,
        )
    };

    // Parse the SVG with font database
    let usvg_options = usvg::Options {
        fontdb,
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&svg_template, &usvg_options)
        .map_err(|e| SvgRenderError::SvgParseError(e.to_string()))?;

    // Get the SVG size and apply scale factor for higher resolution output
    let scale_factor = 2.0; // Scale up for better quality
    let svg_size = tree.size();
    let width = (svg_size.width() * scale_factor) as u32;
    let height = (svg_size.height() * scale_factor) as u32;

    // Create a pixmap to render into
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or(SvgRenderError::PixmapCreationError)?;

    // Render the SVG with scale transform
    let transform = tiny_skia::Transform::from_scale(scale_factor, scale_factor);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Encode as PNG and return the data
    pixmap
        .encode_png()
        .map_err(|e| SvgRenderError::PngSaveError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_user_badge_to_png() {
        let png_data = render_user_badge_to_png("#FF5733", "Alice", false).unwrap();

        // Verify it's valid PNG data by checking PNG signature
        assert_eq!(&png_data[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // Should have some reasonable size (not empty)
        assert!(png_data.len() > 100);

        // Test with different parameters
        let png_data2 = render_user_badge_to_png("#00FF00", "Bob Doe", false).unwrap();
        assert_eq!(&png_data2[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(png_data2.len() > 100);

        // The two images should be different (different color/name)
        assert_ne!(png_data, png_data2);
    }

    #[test]
    fn test_calculate_box_width() {
        // Short names should use base width
        assert_eq!(calculate_box_width("Jo"), 29.0);
        assert_eq!(calculate_box_width("John"), 42.0);
        assert_eq!(calculate_box_width("Alice"), 48.5);

        // Longer names should have increased width
        let long_width = calculate_box_width("Alice & Bob");
        assert!(long_width > 48.5);

        // Very long names should have proportionally wider boxes
        let very_long_width = calculate_box_width("Very Long Username");
        assert!(very_long_width > long_width);

        // Test specific calculations
        assert_eq!(calculate_box_width("1234567"), 29.0 + 5.0 * 6.5); // 7 chars = base_width + (7-2) * 6.5px
    }

    #[test]
    fn test_different_name_lengths() {
        // Test badges with different name lengths (now with dynamic box width)
        let very_short_badge = render_user_badge_to_png("#0040FF", "Me", false).unwrap();
        let short_badge = render_user_badge_to_png("#0040FF", "Joe", false).unwrap();
        let medium_badge = render_user_badge_to_png("#0040FF", "Alice Doe", false).unwrap();
        let long_badge = render_user_badge_to_png("#0040FF", "Iason Parask", false).unwrap();
        let extra_long_badge =
            render_user_badge_to_png("#0040FF", "AlexanderGGGGGGGGGGG", false).unwrap();
        let extra_long_badge_two =
            render_user_badge_to_png("#0040FF", "Lykourgos Mpezentakos", false).unwrap();

        // All should generate valid PNG data
        assert_eq!(&short_badge[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&medium_badge[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&long_badge[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&extra_long_badge[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // Save examples for visual inspection
        std::fs::write("very_short_name_wide_badge.png", very_short_badge).unwrap();
        std::fs::write("short_name_wide_badge.png", short_badge).unwrap();
        std::fs::write("medium_name_wide_badge.png", medium_badge).unwrap();
        std::fs::write("long_name_wide_badge.png", long_badge).unwrap();
        std::fs::write("extra_long_name_wide_badge.png", extra_long_badge).unwrap();
        std::fs::write("extra_long_name_wide_badge_two.png", extra_long_badge_two).unwrap();
    }

    #[test]
    fn test_pointer_badge() {
        // Test the pointer template
        let pointer_badge = render_user_badge_to_png("#0040FF", "Costa", true).unwrap();

        // Verify it's valid PNG data by checking PNG signature
        assert_eq!(&pointer_badge[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // Should have some reasonable size (not empty)
        assert!(pointer_badge.len() > 100);

        // Test regular badge for comparison
        let regular_badge = render_user_badge_to_png("#0040FF", "Costa", false).unwrap();

        // The two images should be different (different templates)
        assert_ne!(pointer_badge, regular_badge);

        // Save example for visual inspection
        std::fs::write("test_pointer_badge.png", pointer_badge).unwrap();
    }
}
