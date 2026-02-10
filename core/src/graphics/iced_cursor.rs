//! SVG-based cursor for iced canvas rendering.
//!
//! This module provides an `IcedCursor` that renders user cursors as SVG graphics
//! on an iced canvas frame. Unlike the PNG-based approach in `svg_renderer.rs`,
//! this uses iced's native SVG support for direct rendering.

use crate::utils::geometry::Position;
use iced::widget::canvas::Frame;
use iced::Rectangle;

/// Calculate dynamic box width based on text length.
/// Increases box width for longer text to ensure it fits comfortably.
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

/// Generate the regular arrow cursor SVG string.
fn generate_normal_cursor_svg(color: &str, name: &str, box_width: f32) -> String {
    let scale_factor = 2.0;
    let text_x_regular = 18.6445 + 6.0;
    let svg_width_regular = (box_width + 36.0) * scale_factor;
    let svg_height_regular = 75.0 * scale_factor;

    format!(
        r##"<svg width="{svg_width}" height="{svg_height}" viewBox="0 0 {svg_width} {svg_height}" fill="none" xmlns="http://www.w3.org/2000/svg">
<g transform="scale({scale_factor})">
<g filter="url(#filter0_di_3982_4518)">
<path d="M11.1115 28.1619C10.3335 29.2773 8.59925 28.9255 8.31748 27.595L3.33755 4.08087C3.06368 2.78771 4.42715 1.76508 5.59191 2.39005L27.3579 14.0689C28.606 14.7386 28.3801 16.5926 27.0078 16.9431L17.7466 19.3083C17.3856 19.4004 17.0699 19.6192 16.8568 19.9248L11.1115 28.1619Z" fill="{color}"/>
<path d="M3.71777 4C3.51267 3.03029 4.53473 2.26375 5.4082 2.73242L27.1738 14.4111C28.1097 14.9133 27.9409 16.3032 26.9121 16.5664L17.6504 18.9316C17.1993 19.0468 16.8045 19.3204 16.5381 19.7021L10.793 27.9395C10.2095 28.776 8.90864 28.5124 8.69727 27.5146L3.71777 4Z" stroke="white" stroke-opacity="1" stroke-width="1"/>
</g>
<g filter="url(#filter1_di_3982_4518)">
<rect x="18.6445" y="22.8086" width="{box_width}" height="35" rx="17.5" fill="{color}"/>
<rect x="19.1949" y="23.3589" width="{box_width_stroke}" height="33.8994" rx="16.9497" stroke="white" stroke-opacity="1" stroke-width="1"/>
<text fill="white" xml:space="preserve" style="white-space: pre" font-family="Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif" font-size="11.606" font-weight="600" letter-spacing="0.05em"><tspan x="{text_x}" y="44.5486">{name}</tspan></text>
</g>
</g>
<defs>
<filter id="filter0_di_3982_4518" x="-0.657412" y="-1.3927" width="34.6039" height="36.6039" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
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
<filter id="filter1_di_3982_4518" x="2.13481" y="6.29888" width="{filter_width}" height="68.0194" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
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
</svg>"##,
        color = color,
        name = name,
        box_width = box_width,
        box_width_stroke = box_width - 1.10065,
        filter_width = box_width + 16.5097 * 2.0,
        text_x = text_x_regular,
        svg_width = svg_width_regular,
        svg_height = svg_height_regular,
        scale_factor = scale_factor,
    )
}

/// Generate the pointer/hand cursor SVG string.
fn generate_pointer_cursor_svg(color: &str, name: &str, box_width: f32) -> String {
    let scale_factor = 2.0;
    let text_x_pointer = 16.5317 + 6.0;
    let svg_width_pointer = (box_width + 34.0) * scale_factor;
    let svg_height_pointer = 74.0 * scale_factor;

    format!(
        r##"<svg width="{svg_width}" height="{svg_height}" viewBox="0 0 {svg_width} {svg_height}" fill="none" xmlns="http://www.w3.org/2000/svg">
<g transform="scale({scale_factor})">
<g filter="url(#filter0_i_3994_1008)">
<g filter="url(#filter1_d_3994_1008)">
<g filter="url(#filter2_i_3994_1008)">
<path d="M2.40749 13.6518L2.7247 15.5709L3.84935 16.464L5.12039 16.951L11.9511 23.7817L20.6231 20.6997L20.8002 17.8562L20.7076 14.2666L18.4636 7.01823L16.5589 6.12507L14.6074 5.94603L10.1285 6.12507L8.20668 6.35776L5.33446 1.74688L3.20274 2.51759L6.98815 14.0294L4.08571 12.6684L2.40749 13.6518Z" fill="{color}"/>
</g>
<path d="M2.18055 3.07546L3.24641 2.6901L7.48533 14.4146L6.41947 14.8L6.03411 13.7341L4.96825 14.1194L4.58289 13.0536L5.64876 12.6682L2.18055 3.07546ZM18.0538 6.97809L20.7513 14.4391L21.8171 14.0538L19.1196 6.59273L18.0538 6.97809ZM0.999948 13.1438L2.15602 16.3414L3.22188 15.956L2.45117 13.8243L4.58289 13.0536L4.19754 11.9877L0.999948 13.1438ZM6.50968 18.3829L5.73896 16.2512L4.6731 16.6365L5.44381 18.7683L6.50968 18.3829ZM8.34625 20.1293L7.57554 17.9975L6.50968 18.3829L7.28039 20.5146L8.34625 20.1293ZM10.1828 21.8756L11.3389 25.0732L21.9975 21.2197L20.8439 18.0287L19.778 18.4141L20.5463 20.5392L12.0194 23.622L11.2487 21.4903L10.1828 21.8756L9.41212 19.7439L8.34625 20.1293L9.11697 22.261L10.1828 21.8756ZM20.8415 18.0221L21.9073 17.6367L20.7513 14.4391L19.6854 14.8245L20.8415 18.0221ZM4.6731 16.6365L4.28774 15.5707L3.22188 15.956L3.60724 17.0219L4.6731 16.6365ZM16.6025 6.29758L16.9879 7.36344L18.0538 6.97809L17.6684 5.91222L16.6025 6.29758ZM14.0855 6.00243L15.6269 10.2659L16.6927 9.88053L15.5367 6.68294L16.6025 6.29758L16.2172 5.23172L14.0855 6.00243ZM10.5025 6.09264L12.0439 10.3561L13.1098 9.97073L11.9537 6.77314L14.0855 6.00243L13.7001 4.93657L10.5025 6.09264ZM7.98543 5.79749L6.444 1.53403L5.37814 1.91939L8.84635 11.5122L9.91221 11.1268L8.37078 6.86335L10.5025 6.09264L10.1172 5.02677L7.98543 5.79749ZM2.86105 1.62424L3.24641 2.6901L5.37814 1.91939L4.99278 0.853529L2.86105 1.62424Z" fill="white"/>
</g>
</g>
<g filter="url(#filter3_di_3994_1008)">
<rect x="16.5317" y="22" width="{box_width}" height="35" rx="17.5" fill="{color}"/>
<rect x="17.0821" y="22.5503" width="{box_width_stroke}" height="33.8994" rx="16.9497" stroke="white" stroke-opacity="0.7" stroke-width="1.10065"/>
<text fill="white" xml:space="preserve" style="white-space: pre" font-family="Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif" font-size="11.606" font-weight="600" letter-spacing="0.05em"><tspan x="{text_x}" y="43.74">{name}</tspan></text>
</g>
</g>
<defs>
<filter id="filter0_i_3994_1008" x="0.531738" y="0" width="30.3736" height="33.1648" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="2.37363" dy="3.16484"/>
<feGaussianBlur stdDeviation="3.95604"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.4 0"/>
<feBlend mode="normal" in2="shape" result="effect1_innerShadow_3994_1008"/>
</filter>
<filter id="filter1_d_3994_1008" x="-0.468262" y="0" width="30" height="32" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="1"/>
<feGaussianBlur stdDeviation="0.5"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.1 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3994_1008"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3994_1008" result="shape"/>
</filter>
<filter id="filter2_i_3994_1008" x="2.40747" y="1.74683" width="20.2383" height="24.4955" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="1.84544" dy="2.46058"/>
<feGaussianBlur stdDeviation="3.07573"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.4 0"/>
<feBlend mode="normal" in2="shape" result="effect1_innerShadow_3994_1008"/>
</filter>
<filter id="filter3_di_3994_1008" x="0.0220203" y="5.49028" width="{filter_width}" height="68.0194" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset/>
<feGaussianBlur stdDeviation="8.25486"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.1 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_3994_1008"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_3994_1008" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="3.16484"/>
<feGaussianBlur stdDeviation="4.35165"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.75 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow_3994_1008"/>
</filter>
</defs>
</svg>"##,
        color = color,
        name = name,
        box_width = box_width,
        box_width_stroke = box_width - 1.10065,
        filter_width = box_width + 16.5097 * 2.0,
        text_x = text_x_pointer,
        svg_width = svg_width_pointer,
        svg_height = svg_height_pointer,
        scale_factor = scale_factor,
    )
}

/// An SVG-based cursor for rendering on iced canvas frames.
///
/// This struct holds both normal (arrow) and pointer (hand) cursor variants
/// as SVG handles that can be rendered directly onto an iced canvas frame.
#[derive(Debug)]
pub struct IcedCursor {
    /// Hex color code for the cursor (stored for future use)
    #[allow(dead_code)]
    color: String,
    /// SVG handle for normal arrow cursor
    normal_cursor: iced_core::svg::Handle,
    /// SVG handle for pointer/hand cursor
    pointer_cursor: iced_core::svg::Handle,
    /// Logical width of cursor (same for both variants in this implementation)
    cursor_width: f32,
    /// Logical height of cursor (same for both variants in this implementation)
    cursor_height: f32,
    /// Current position of the cursor
    position: Option<Position>,
}

impl IcedCursor {
    /// Creates a new `IcedCursor` with the given color and name.
    ///
    /// # Arguments
    ///
    /// * `color` - Hex color code (e.g., "#FF5733")
    /// * `name` - Name text to display on the cursor badge
    ///
    /// # Returns
    ///
    /// A new `IcedCursor` instance with both normal and pointer SVG handles initialized.
    pub fn new(color: &str, name: &str) -> Self {
        // Calculate box width and clamp/truncate if necessary
        let mut box_width = calculate_box_width(name);
        let mut name = name.to_string();

        if box_width > 152.0 {
            box_width = 152.0;
            name = name.chars().take(17).collect::<String>() + "...";
        }

        // Generate SVG strings for both cursor types
        let normal_svg = generate_normal_cursor_svg(color, &name, box_width);
        let pointer_svg = generate_pointer_cursor_svg(color, &name, box_width);

        // Create SVG handles from memory
        let normal_cursor = iced_core::svg::Handle::from_memory(normal_svg.into_bytes());
        let pointer_cursor = iced_core::svg::Handle::from_memory(pointer_svg.into_bytes());

        // Calculate logical dimensions (using the regular cursor dimensions as reference)
        let cursor_width = box_width + 36.0;
        let cursor_height = 75.0;

        Self {
            color: color.to_string(),
            normal_cursor,
            pointer_cursor,
            cursor_width,
            cursor_height,
            position: None,
        }
    }

    /// Sets the cursor position.
    ///
    /// # Arguments
    ///
    /// * `position` - The new position for the cursor
    pub fn set_position(&mut self, position: Option<Position>) {
        self.position = position;
    }

    /// Draws the cursor onto an iced canvas frame.
    ///
    /// # Arguments
    ///
    /// * `frame` - The iced canvas frame to draw onto
    /// * `pointer` - If true, draws the pointer/hand cursor; otherwise draws the normal arrow cursor
    pub fn draw(&self, frame: &mut Frame, pointer: bool) {
        if self.position.is_none() {
            return;
        }

        let handle = if pointer {
            &self.pointer_cursor
        } else {
            &self.normal_cursor
        };

        let svg = iced_core::svg::Svg::new(handle.clone());
        let position = self.position.as_ref().unwrap();
        frame.draw_svg(
            Rectangle {
                x: position.x as f32,
                y: position.y as f32,
                width: self.cursor_width,
                height: self.cursor_height,
            },
            svg,
        );
    }
}
