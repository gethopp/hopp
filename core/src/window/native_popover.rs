/// Native macOS NSPopover dropdown with frosted-glass blur background.
///
/// Replaces the iced-rendered dropdown with a real AppKit popover so the
/// desktop content behind the window blurs through, matching the system
/// appearance of macOS popovers (e.g. Control Center, menu extras).
///
/// Communication back to the iced event loop uses two shared atomics:
/// - `popover_selection` (`AtomicU8`):  0 = none, 1 = "Fade Out", 2 = "Persist"
/// - `popover_open` (`AtomicBool`): true while the popover is visible
///
/// The screensharing window polls these on each `RedrawRequested`.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSAppearance, NSBitmapImageRep, NSButton, NSButtonType, NSColor, NSDeviceRGBColorSpace, NSFont,
    NSImage, NSImageRep, NSImageScaling, NSImageView, NSPopover, NSPopoverBehavior, NSStackView,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use fontdb::Database;
use resvg::{tiny_skia, usvg};
use winit::window::Window;

const POPOVER_WIDTH: f64 = 248.0;
const POPOVER_HEIGHT: f64 = 76.0;
const ITEM_HEIGHT: f64 = 32.0;
const ICON_SIZE: f64 = 16.0;

const PENCIL_SVG: &[u8] = include_bytes!("../../resources/icons/pencil.svg");

// ── PopoverTarget ───────────────────────────────────────────────────────────

struct PopoverTargetIvars {
    selection: Arc<AtomicU8>,
    popover_open: Arc<AtomicBool>,
    item_index: u8,
    popover: Retained<NSPopover>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "HoppPopoverTarget"]
    #[ivars = PopoverTargetIvars]
    struct PopoverTarget;

    impl PopoverTarget {
        #[unsafe(method(handleClick:))]
        fn handle_click(&self, _sender: Option<&AnyObject>) {
            let ivars = self.ivars();
            ivars
                .selection
                .store(ivars.item_index + 1, Ordering::Relaxed);
            ivars.popover_open.store(false, Ordering::Relaxed);
            ivars.popover.close();
        }

        /// Called by NSNotificationCenter when the popover closes for any
        /// reason (item click, outside click, Escape key, etc.).
        /// Source:
        /// https://developer.apple.com/documentation/safariservices/sfsafariextensionhandling/popoverdidclose(in:)
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: Option<&AnyObject>) {
            self.ivars().popover_open.store(false, Ordering::Relaxed);
        }
    }
);

impl PopoverTarget {
    fn new(
        mtm: MainThreadMarker,
        selection: Arc<AtomicU8>,
        popover_open: Arc<AtomicBool>,
        item_index: u8,
        popover: Retained<NSPopover>,
    ) -> Retained<Self> {
        let this = mtm.alloc::<PopoverTarget>();
        let this = this.set_ivars(PopoverTargetIvars {
            selection,
            popover_open,
            item_index,
            popover,
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Show a native macOS NSPopover anchored near the cog button of the
/// screensharing window. The popover uses the system popover material
/// for a frosted-glass blur, and contains two clickable items.
///
/// `draw_persist` determines which item has a checkmark.
/// `selection` is the shared atomic that receives the user's choice.
pub fn show_settings_popover(
    window: &Window,
    draw_persist: bool,
    selection: Arc<AtomicU8>,
    popover_open: Arc<AtomicBool>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("show_settings_popover: not on main thread, skipping");
        return;
    };

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("show_settings_popover: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("show_settings_popover: not an AppKit handle");
        return;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("show_settings_popover: failed to retain NSView");
            return;
        };

        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAnimates(true);
        popover.setContentSize(NSSize::new(POPOVER_WIDTH, POPOVER_HEIGHT));

        let dark_name = objc2_foundation::ns_string!("NSAppearanceNameDarkAqua");
        if let Some(dark_appearance) = NSAppearance::appearanceNamed(dark_name) {
            let _: () = msg_send![&*popover, setAppearance: &*dark_appearance];
        }

        let pencil_image = rasterize_svg_to_nsimage(PENCIL_SVG, ICON_SIZE);

        popover_open.store(true, Ordering::Relaxed);

        let target0 = PopoverTarget::new(
            mtm,
            selection.clone(),
            popover_open.clone(),
            0,
            popover.clone(),
        );
        let target1 = PopoverTarget::new(mtm, selection, popover_open, 1, popover.clone());

        // Observe NSPopoverDidCloseNotification so we detect dismiss-without-click.
        let nc: Retained<AnyObject> = msg_send![objc2::class!(NSNotificationCenter), defaultCenter];
        let notif_name = NSString::from_str("NSPopoverDidCloseNotification");
        let _: () = msg_send![
            &*nc,
            addObserver: &*target0,
            selector: sel!(popoverDidClose:),
            name: &*notif_name,
            object: &*popover
        ];

        let row0 = make_menu_row(mtm, "Fade Out", !draw_persist, &pencil_image, &target0);
        let row1 = make_menu_row(
            mtm,
            "Persist Until Right Click",
            draw_persist,
            &pencil_image,
            &target1,
        );

        let stack = NSStackView::new(mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        let _: () = msg_send![&*stack, setSpacing: 2.0_f64];
        let _: () = msg_send![&*stack, setEdgeInsets: objc2_foundation::NSEdgeInsets {
            top: 4.0,
            left: 4.0,
            bottom: 4.0,
            right: 4.0,
        }];
        stack.addArrangedSubview(&row0);
        stack.addArrangedSubview(&row1);

        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(POPOVER_WIDTH, POPOVER_HEIGHT),
        );
        let _: () = msg_send![&*stack, setFrame: frame];

        let vc = NSViewController::new(mtm);
        vc.setView(&stack);
        popover.setContentViewController(Some(&vc));

        // The winit content view uses a flipped coordinate system (y=0 at top).
        // Anchor at the cog button position: top-right of the header.
        let view_bounds = ns_view.bounds();
        let anchor_w = 44.0_f64;
        let anchor_h = 24.0_f64;
        let right_pad = 4.0_f64;
        let top_pad = 4.0_f64;
        let anchor_rect = NSRect::new(
            NSPoint::new(view_bounds.size.width - anchor_w - right_pad, top_pad),
            NSSize::new(anchor_w, anchor_h),
        );

        popover.showRelativeToRect_ofView_preferredEdge(
            anchor_rect,
            &ns_view,
            objc2_foundation::NSRectEdge::MaxY,
        );

        // Keep targets alive while the popover is visible via associated objects.
        use objc2::ffi::objc_setAssociatedObject;
        use objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC;
        static mut KEY0: u8 = 0;
        static mut KEY1: u8 = 0;
        objc_setAssociatedObject(
            (&*popover as *const NSPopover).cast_mut().cast(),
            std::ptr::addr_of_mut!(KEY0).cast(),
            (&*target0 as *const PopoverTarget).cast_mut().cast(),
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
        objc_setAssociatedObject(
            (&*popover as *const NSPopover).cast_mut().cast(),
            std::ptr::addr_of_mut!(KEY1).cast(),
            (&*target1 as *const PopoverTarget).cast_mut().cast(),
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );

        log::info!("show_settings_popover: popover shown");
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

const CHECK_COL_W: f64 = 18.0;
const ICON_COL_W: f64 = 22.0;
const COL_SPACING: f64 = 4.0;
const ROW_LEFT_PAD: f64 = 8.0;

/// Build one menu row as a borderless NSButton whose content view is a
/// horizontal stack: `[checkmark 18pt] [icon 22pt] [label fill]`.
/// All three columns are always present so text stays aligned across rows.
unsafe fn make_menu_row(
    mtm: MainThreadMarker,
    label: &str,
    selected: bool,
    icon_image: &NSImage,
    target: &PopoverTarget,
) -> Retained<NSView> {
    let row_w = POPOVER_WIDTH - 8.0;

    // ── Checkmark column (fixed width) ───────────────────────────────
    let check_text = if selected { "\u{2713}" } else { "" };
    let check_label = NSTextField::labelWithString(&NSString::from_str(check_text), mtm);
    let _: () = msg_send![&*check_label, setBezeled: false];
    let _: () = msg_send![&*check_label, setDrawsBackground: false];
    let _: () = msg_send![&*check_label, setEditable: false];
    let _: () = msg_send![&*check_label, setSelectable: false];
    check_label.setTextColor(Some(&NSColor::whiteColor()));
    let check_font = NSFont::systemFontOfSize(13.0);
    let _: () = msg_send![&*check_label, setFont: &*check_font];
    let _: () = msg_send![&*check_label, setAlignment: 1_isize]; // NSTextAlignmentCenter
    let check_frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(CHECK_COL_W, ITEM_HEIGHT),
    );
    let _: () = msg_send![&*check_label, setFrame: check_frame];

    // ── Icon column (fixed width) ────────────────────────────────────
    let icon_view = NSImageView::new(mtm);
    icon_view.setImage(Some(icon_image));
    icon_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    let icon_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ICON_COL_W, ITEM_HEIGHT));
    let _: () = msg_send![&*icon_view, setFrame: icon_frame];

    // ── Label column (fills remaining width) ─────────────────────────
    let label_field = NSTextField::labelWithString(&NSString::from_str(label), mtm);
    let _: () = msg_send![&*label_field, setBezeled: false];
    let _: () = msg_send![&*label_field, setDrawsBackground: false];
    let _: () = msg_send![&*label_field, setEditable: false];
    let _: () = msg_send![&*label_field, setSelectable: false];
    label_field.setTextColor(Some(&NSColor::whiteColor()));
    let label_font = NSFont::systemFontOfSize(13.0);
    let _: () = msg_send![&*label_field, setFont: &*label_font];
    let _: () = msg_send![&*label_field, setAlignment: 0_isize]; // NSTextAlignmentLeft
    let _: () = msg_send![&*label_field, setLineBreakMode: 5_isize]; // NSLineBreakByTruncatingTail

    // ── Horizontal stack: [check] [icon] [label] ─────────────────────
    let row_stack = NSStackView::new(mtm);
    row_stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    let _: () = msg_send![&*row_stack, setSpacing: COL_SPACING];
    let _: () = msg_send![&*row_stack, setEdgeInsets: objc2_foundation::NSEdgeInsets {
        top: 0.0,
        left: ROW_LEFT_PAD,
        bottom: 0.0,
        right: 8.0,
    }];
    row_stack.addArrangedSubview(&check_label);
    row_stack.addArrangedSubview(&icon_view);
    row_stack.addArrangedSubview(&label_field);

    let row_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(row_w, ITEM_HEIGHT));
    let _: () = msg_send![&*row_stack, setFrame: row_frame];

    // Pin fixed widths via auto-layout constraints.
    let _: () = msg_send![&*check_label, setTranslatesAutoresizingMaskIntoConstraints: false];
    let _: () = msg_send![&*icon_view, setTranslatesAutoresizingMaskIntoConstraints: false];
    let _: () = msg_send![&*label_field, setTranslatesAutoresizingMaskIntoConstraints: false];

    let check_w_constraint: Retained<objc2_app_kit::NSLayoutConstraint> =
        msg_send![&*check_label, widthAnchor];
    let check_constraint: Retained<objc2_app_kit::NSLayoutConstraint> = msg_send![
        &*check_w_constraint, constraintEqualToConstant: CHECK_COL_W
    ];
    let _: () = msg_send![&*check_constraint, setActive: true];

    let icon_w_anchor: Retained<objc2_app_kit::NSLayoutConstraint> =
        msg_send![&*icon_view, widthAnchor];
    let icon_constraint: Retained<objc2_app_kit::NSLayoutConstraint> = msg_send![
        &*icon_w_anchor, constraintEqualToConstant: ICON_COL_W
    ];
    let _: () = msg_send![&*icon_constraint, setActive: true];

    // ── Wrap in a borderless button for click handling ────────────────
    let btn = NSButton::new(mtm);
    btn.setButtonType(NSButtonType::MomentaryPushIn);
    btn.setTarget(Some(target));
    btn.setAction(Some(sel!(handleClick:)));
    btn.setTitle(&NSString::from_str(""));
    let _: () = msg_send![&*btn, setBordered: false];
    // NSImageOnly = 1 — hides the empty title, keeps the button transparent
    let _: () = msg_send![&*btn, setImagePosition: 1_isize];
    let _: () = msg_send![&*btn, setFrame: row_frame];

    // Container view: stack + button overlaid
    let container = NSView::new(mtm);
    let _: () = msg_send![&*container, setFrame: row_frame];
    container.addSubview(&row_stack);
    container.addSubview(&btn);

    // Pin container height so the outer vertical stack doesn't collapse it.
    let _: () = msg_send![&*container, setTranslatesAutoresizingMaskIntoConstraints: false];
    let container_h_anchor: Retained<objc2_app_kit::NSLayoutDimension> =
        msg_send![&*container, heightAnchor];
    let h_constraint: Retained<objc2_app_kit::NSLayoutConstraint> = msg_send![
        &*container_h_anchor, constraintEqualToConstant: ITEM_HEIGHT
    ];
    let _: () = msg_send![&*h_constraint, setActive: true];

    let container_w_anchor: Retained<objc2_app_kit::NSLayoutDimension> =
        msg_send![&*container, widthAnchor];
    let w_constraint: Retained<objc2_app_kit::NSLayoutConstraint> = msg_send![
        &*container_w_anchor, constraintEqualToConstant: row_w
    ];
    let _: () = msg_send![&*w_constraint, setActive: true];

    container
}

/// Rasterize an SVG to an NSImage at the given logical point size.
fn rasterize_svg_to_nsimage(svg_bytes: &[u8], point_size: f64) -> Retained<NSImage> {
    let px_size = (point_size * 2.0) as u32; // 2x for Retina

    let fontdb = std::sync::Arc::new(Database::new());
    let usvg_options = usvg::Options {
        fontdb,
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(svg_bytes, &usvg_options)
        .expect("rasterize_svg_to_nsimage: failed to parse SVG");
    let svg_size = tree.size();
    let max_dim = svg_size.width().max(svg_size.height());
    let scale = if max_dim > 0.0 {
        px_size as f32 / max_dim
    } else {
        1.0
    };
    let w = (svg_size.width() * scale).ceil().max(1.0) as u32;
    let h = (svg_size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).expect("rasterize_svg_to_nsimage: pixmap creation failed");
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // Convert premultiplied RGBA to straight alpha.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as f32;
        if a > 0.0 && a < 255.0 {
            let inv = 255.0 / a;
            px[0] = (px[0] as f32 * inv).round().min(255.0) as u8;
            px[1] = (px[1] as f32 * inv).round().min(255.0) as u8;
            px[2] = (px[2] as f32 * inv).round().min(255.0) as u8;
        }
    }

    unsafe {
        let planes_ptr: *mut *mut u8 = std::ptr::null_mut();
        let rep: Retained<NSBitmapImageRep> = msg_send![
            NSBitmapImageRep::alloc(),
            initWithBitmapDataPlanes: planes_ptr,
            pixelsWide: w as isize,
            pixelsHigh: h as isize,
            bitsPerSample: 8_isize,
            samplesPerPixel: 4_isize,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: NSDeviceRGBColorSpace,
            bytesPerRow: (w * 4) as isize,
            bitsPerPixel: 32_isize
        ];

        let bitmap_data: *mut u8 = msg_send![&rep, bitmapData];
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), bitmap_data, rgba.len());

        let image = NSImage::new();
        let rep_ref: &NSImageRep =
            &*((&rep as &NSBitmapImageRep) as *const NSBitmapImageRep as *const NSImageRep);
        image.addRepresentation(rep_ref);
        image.setSize(NSSize::new(point_size, point_size));

        image
    }
}
