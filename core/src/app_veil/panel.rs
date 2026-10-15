use std::{ffi::c_void, sync::Once};

use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    data::CFData,
};
use objc2::{rc::Retained, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSBox,
    NSBoxType, NSButton, NSButtonType, NSColor, NSFont, NSImage, NSImageScaling, NSTextField,
    NSTitlePosition,
};
use objc2_foundation::{NSData, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::{components::fonts::GEIST_MEDIUM_BYTES, windows::colors::ColorToken};

pub(super) const WIDTH: f64 = 184.0;
pub(super) const HEIGHT: f64 = 36.0;
pub(super) const INSET: f64 = 12.0;

pub(super) struct EyeButtonIvars {
    toggle_visibility: Box<dyn Fn()>,
    hidden_image: Retained<NSImage>,
    visible_image: Retained<NSImage>,
    status: Retained<NSTextField>,
}

objc2::define_class!(
    #[unsafe(super = NSButton)]
    #[thread_kind = objc2::MainThreadOnly]
    #[ivars = EyeButtonIvars]
    pub(super) struct AppVeilEyeButton;

    unsafe impl NSObjectProtocol for AppVeilEyeButton {}

    impl AppVeilEyeButton {
        #[unsafe(method(toggleVisibility:))]
        fn toggle_visibility(&self, _sender: &NSButton) {
            (self.ivars().toggle_visibility)();
        }
    }
);

impl AppVeilEyeButton {
    fn new(
        mtm: MainThreadMarker,
        status: Retained<NSTextField>,
        toggle_visibility: Box<dyn Fn()>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(EyeButtonIvars {
            toggle_visibility,
            hidden_image: eye_image(include_bytes!("../../resources/icons/EyeOff.svg")),
            visible_image: eye_image(include_bytes!("../../resources/icons/EyeOn.svg")),
            status,
        });
        let this: Retained<Self> = unsafe {
            objc2::msg_send![super(this), initWithFrame: NSRect::new(
                NSPoint::new(WIDTH - 34.0, 4.0), NSSize::new(28.0, 28.0)
            )]
        };
        this.setTitle(&NSString::from_str(""));
        this.setButtonType(NSButtonType::MomentaryChange);
        this.setBordered(false);
        this.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        unsafe {
            // The button owns itself through the content view; the target is weak.
            this.setTarget(Some(&this));
            this.setAction(Some(objc2::sel!(toggleVisibility:)));
        }
        this.update_appearance(false);
        this
    }

    pub(super) fn update_appearance(&self, visible: bool) {
        self.ivars()
            .status
            .setStringValue(&NSString::from_str(if visible {
                "Visible"
            } else {
                "Hidden"
            }));
        self.ivars().status.setTextColor(Some(&color(if visible {
            ColorToken::Orange400
        } else {
            ColorToken::Gray400
        })));
        let image = if visible {
            &self.ivars().visible_image
        } else {
            &self.ivars().hidden_image
        };
        self.setImage(Some(image));
        self.setContentTintColor(Some(&color(if visible {
            ColorToken::Orange400
        } else {
            ColorToken::Gray100
        })));
        let description = NSString::from_str(if visible {
            "Visible to others · Click to hide"
        } else {
            "Hidden from others · Click to show"
        });
        self.setAccessibilityLabel(Some(&description));
    }
}

pub(super) fn content(
    mtm: MainThreadMarker,
    toggle_visibility: Box<dyn Fn()>,
) -> (Retained<NSBox>, Retained<AppVeilEyeButton>) {
    register_font();
    let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT));
    let background = NSBox::initWithFrame(NSBox::alloc(mtm), bounds);
    background.setBoxType(NSBoxType::Custom);
    background.setTitlePosition(NSTitlePosition::NoTitle);
    background.setContentViewMargins(NSSize::new(0.0, 0.0));
    background.setBorderWidth(1.0);
    background.setCornerRadius(10.0);
    background.setBorderColor(&color(ColorToken::Slate600));
    background.setFillColor(&color(ColorToken::Zinc900));
    background.setAppearance(
        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref(),
    );

    let label = NSTextField::labelWithString(&NSString::from_str("Hopp App Veil"), mtm);
    let font = NSFont::fontWithName_size(&NSString::from_str("Geist-Medium"), 12.0)
        .unwrap_or_else(|| NSFont::systemFontOfSize(12.0));
    label.setFont(Some(&font));
    label.setTextColor(Some(&color(ColorToken::Gray300)));
    label.setFrame(NSRect::new(
        NSPoint::new(12.0, 10.0),
        NSSize::new(84.0, 16.0),
    ));
    let status = NSTextField::labelWithString(&NSString::from_str("Hidden"), mtm);
    status.setFont(Some(&font));
    status.setFrame(NSRect::new(
        NSPoint::new(96.0, 10.0),
        NSSize::new(40.0, 16.0),
    ));
    let button = AppVeilEyeButton::new(mtm, status.clone(), toggle_visibility);
    if let Some(view) = background.contentView() {
        view.addSubview(&label);
        view.addSubview(&status);
        // Use an explicit fill so AppKit's inactive bezel styling cannot hide the button.
        let button_background = NSBox::initWithFrame(
            NSBox::alloc(mtm),
            NSRect::new(NSPoint::new(WIDTH - 34.0, 4.0), NSSize::new(28.0, 28.0)),
        );
        button_background.setBoxType(NSBoxType::Custom);
        button_background.setTitlePosition(NSTitlePosition::NoTitle);
        button_background.setCornerRadius(8.0);
        button_background.setBorderWidth(1.0);
        button_background.setBorderColor(&color(ColorToken::Slate400));
        button_background.setFillColor(&color(ColorToken::Slate600));
        view.addSubview(&button_background);
        view.addSubview(&button);
    }
    (background, button)
}

fn color(token: ColorToken) -> Retained<NSColor> {
    let color = token.to_color();
    NSColor::colorWithSRGBRed_green_blue_alpha(
        color.r as f64,
        color.g as f64,
        color.b as f64,
        color.a as f64,
    )
}

fn eye_image(svg: &[u8]) -> Retained<NSImage> {
    let (rgba, width, height) = crate::window::drawing_helpers::rasterize_svg_to_rgba(svg, 40);
    let png =
        image::RgbaImage::from_raw(width, height, rgba).expect("embedded eye icon dimensions");
    let mut bytes = std::io::Cursor::new(Vec::new());
    png.write_to(&mut bytes, image::ImageFormat::Png)
        .expect("embedded eye icon PNG");
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes.get_ref()))
        .expect("embedded eye icon image");
    image.setSize(NSSize::new(20.0, 20.0));
    image.setTemplate(true);
    image
}

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    fn CGDataProviderCreateWithCFData(data: CFTypeRef) -> CFTypeRef;
    fn CGFontCreateWithDataProvider(provider: CFTypeRef) -> CFTypeRef;
    fn CTFontManagerRegisterGraphicsFont(font: CFTypeRef, error: *mut *const c_void) -> bool;
}

fn register_font() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let data = CFData::from_buffer(GEIST_MEDIUM_BYTES);
        let provider = CGDataProviderCreateWithCFData(data.as_CFTypeRef());
        if provider.is_null() {
            return;
        }
        let provider = CFType::wrap_under_create_rule(provider);
        let font = CGFontCreateWithDataProvider(provider.as_CFTypeRef());
        if font.is_null() {
            return;
        }
        let font = CFType::wrap_under_create_rule(font);
        if !CTFontManagerRegisterGraphicsFont(font.as_CFTypeRef(), std::ptr::null_mut()) {
            log::debug!("App Veil: Geist registration unavailable; using the native font fallback");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSFontAttributeName, NSStringDrawing};
    use objc2_foundation::NSDictionary;

    #[test]
    fn native_panel_assets_load_with_geist_and_both_eye_states() {
        register_font();
        let font = NSFont::fontWithName_size(&NSString::from_str("Geist-Medium"), 12.0)
            .expect("embedded Geist font is registered with AppKit");
        let attributes =
            NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[&*font as &AnyObject]);
        let label_size =
            unsafe { NSString::from_str("Hopp App Veil").sizeWithAttributes(Some(&attributes)) };
        assert!(
            label_size.width <= 84.0,
            "panel label must fit without truncation"
        );
        for state in ["Hidden", "Visible"] {
            let size = unsafe { NSString::from_str(state).sizeWithAttributes(Some(&attributes)) };
            assert!(
                size.width <= 40.0,
                "status must fit without truncation: {state}"
            );
        }
        for svg in [
            include_bytes!("../../resources/icons/EyeOff.svg").as_slice(),
            include_bytes!("../../resources/icons/EyeOn.svg").as_slice(),
        ] {
            let image = eye_image(svg);
            assert_eq!(image.size(), NSSize::new(20.0, 20.0));
            assert!(image.isTemplate());
        }
    }
}
