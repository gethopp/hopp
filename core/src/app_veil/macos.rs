use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ffi::c_void,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_foundation_sys::base::{CFGetTypeID, CFTypeID, CFTypeRef};
use core_graphics::{
    display::CGDisplay,
    geometry::{CGPoint, CGRect, CGSize},
    window::{
        copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionAll,
        kCGWindowListOptionOnScreenOnly, kCGWindowNumber, kCGWindowOwnerPID,
    },
};
use objc2::{
    rc::{autoreleasepool, Retained},
    MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSRunningApplication,
    NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use winit::event_loop::EventLoopProxy;

use crate::{
    app_veil_state::{AppVeilState, TrackedApplication, TrackedWindow},
    capture::running_applications_observer::RunningApplicationsObserver,
    room_service::{AppVeilSnapshot, AppVeilWindow, NormalizedRect},
    subtract_frame,
    utils::geometry::{Extent, Frame},
    visible_window_fragments, SelectableWindow, UserEvent,
};

#[path = "panel.rs"]
mod panel;

const HOPP_BUNDLE_ID: &str = "com.hopp.app";
const NOTIFICATION_CENTER_BUNDLE_ID: &str = "com.apple.notificationcenterui";
const GEOMETRY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const PANEL_SETTLE_DELAY: Duration = Duration::from_millis(200);
const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
const AX_COPY_STOP_ON_ERROR: u32 = 1;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;
static NEXT_GEOMETRY_POLLER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct PanelAnchor {
    window_id: u32,
    frame: Frame,
}

#[derive(Debug, Clone, Default)]
pub struct HostSnapshot {
    windows: Vec<HostWindow>,
    panels: Vec<PanelAnchor>,
    applications: Option<Vec<TrackedApplication>>,
    last_revealed_window_ids: Option<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq)]
struct HostWindow {
    id: u32,
    bundle_id: String,
    controller: AppVeilWindow,
}

impl HostSnapshot {
    fn retain_bundle_ids(&mut self, bundle_ids: &[String]) {
        let previous_window_count = self.windows.len();
        self.windows
            .retain(|window| bundle_ids.contains(&window.bundle_id));
        if self.windows.len() != previous_window_count {
            self.last_revealed_window_ids = None;
        }
        self.panels.retain(|anchor| {
            self.windows
                .iter()
                .any(|window| window.id == anchor.window_id)
        });
    }

    fn controller_snapshot(&self, revealed_window_ids: &[u32]) -> AppVeilSnapshot {
        AppVeilSnapshot {
            windows: self
                .windows
                .iter()
                .filter(|window| !revealed_window_ids.contains(&window.id))
                .map(|window| window.controller.clone())
                .collect(),
            keyboard_input_blocked: false,
        }
    }

    fn changed_controller_snapshot(
        &mut self,
        revealed_window_ids: Vec<u32>,
    ) -> Option<AppVeilSnapshot> {
        if self.last_revealed_window_ids.as_ref() == Some(&revealed_window_ids) {
            return None;
        }
        let snapshot = self.controller_snapshot(&revealed_window_ids);
        self.last_revealed_window_ids = Some(revealed_window_ids);
        Some(snapshot)
    }
}

enum GeometryPollerMessage {
    SetBundleIds {
        revision: u64,
        bundle_ids: Vec<String>,
    },
    Stop,
}

struct GeometryPoller {
    id: u64,
    revision: u64,
    sender: mpsc::Sender<GeometryPollerMessage>,
    thread: Option<JoinHandle<()>>,
}

impl GeometryPoller {
    fn new(
        display_id: u32,
        bundle_ids: Vec<String>,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let id = NEXT_GEOMETRY_POLLER_ID.fetch_add(1, Ordering::Relaxed);
        let mut geometry_active = None;
        let mut qualified_windows = HashMap::new();
        Self::spawn(
            id,
            display_id,
            bundle_ids,
            GEOMETRY_POLL_INTERVAL,
            move |revision, display_id, bundle_ids| {
                autoreleasepool(|_| {
                    let is_active = has_running_protected_application(bundle_ids);
                    let state_changed = geometry_active != Some(is_active);
                    if state_changed {
                        geometry_active = Some(is_active);
                    }
                    if !is_active {
                        qualified_windows.clear();
                        if state_changed {
                            let _ = event_loop_proxy.send_event(UserEvent::PolledAppVeilSnapshot(
                                id,
                                revision,
                                HostSnapshot {
                                    applications: tracked_applications(bundle_ids),
                                    ..HostSnapshot::default()
                                },
                            ));
                        }
                        return;
                    }
                    if let Some(snapshot) = snapshot_with_qualified_windows(
                        display_id,
                        bundle_ids,
                        &mut qualified_windows,
                    ) {
                        let _ = event_loop_proxy
                            .send_event(UserEvent::PolledAppVeilSnapshot(id, revision, snapshot));
                    }
                });
            },
        )
    }

    fn spawn(
        id: u64,
        display_id: u32,
        mut bundle_ids: Vec<String>,
        interval: Duration,
        mut callback: impl FnMut(u64, u32, &[String]) + Send + 'static,
    ) -> Self {
        let mut revision = 0;
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            callback(revision, display_id, &bundle_ids);
            loop {
                match receiver.recv_timeout(interval) {
                    Ok(GeometryPollerMessage::SetBundleIds {
                        revision: updated_revision,
                        bundle_ids: updated_bundle_ids,
                    }) => {
                        revision = updated_revision;
                        bundle_ids = updated_bundle_ids;
                        callback(revision, display_id, &bundle_ids);
                    }
                    Ok(GeometryPollerMessage::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if geometry_enabled(&bundle_ids) {
                            callback(revision, display_id, &bundle_ids);
                        }
                    }
                }
            }
        });
        Self {
            id,
            revision: 0,
            sender,
            thread: Some(thread),
        }
    }

    fn set_bundle_ids(&mut self, bundle_ids: Vec<String>) {
        self.revision = self.revision.wrapping_add(1);
        let _ = self.sender.send(GeometryPollerMessage::SetBundleIds {
            revision: self.revision,
            bundle_ids,
        });
    }
}

impl Drop for GeometryPoller {
    fn drop(&mut self) {
        let _ = self.sender.send(GeometryPollerMessage::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Window {
    id: u32,
    frame: Frame,
    bundle_id: Option<String>,
}

pub struct AppVeilHost {
    state: Arc<Mutex<AppVeilState>>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    geometry_poller: GeometryPoller,
    _running_applications_observer: RunningApplicationsObserver,
    panels: HashMap<u32, HostPanel>,
    snapshot: HostSnapshot,
}

struct HostPanel {
    window: Retained<NSPanel>,
    button: Retained<panel::AppVeilEyeButton>,
    active: Rc<Cell<bool>>,
    last_moved_at: Option<Instant>,
}

fn panel_is_settled(last_moved_at: Option<Instant>, now: Instant) -> bool {
    last_moved_at.is_none_or(|last_moved| now.duration_since(last_moved) >= PANEL_SETTLE_DELAY)
}

impl Drop for HostPanel {
    fn drop(&mut self) {
        self.active.set(false);
        self.window.close();
    }
}

impl AppVeilHost {
    pub fn new(
        display_id: u32,
        bundle_ids: Vec<String>,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Option<Self> {
        let state = Arc::new(Mutex::new(AppVeilState::new(bundle_ids.clone())));
        let running_applications_observer =
            RunningApplicationsObserver::new(event_loop_proxy.clone(), state.clone())?;
        let geometry_poller = GeometryPoller::new(display_id, bundle_ids, event_loop_proxy.clone());
        Some(Self {
            state,
            event_loop_proxy,
            geometry_poller,
            _running_applications_observer: running_applications_observer,
            panels: HashMap::new(),
            snapshot: HostSnapshot::default(),
        })
    }

    pub fn set_bundle_ids(&mut self, bundle_ids: Vec<String>) {
        let mut state = self.state.lock().unwrap();
        let current: Vec<_> = state
            .applications
            .iter()
            .map(|application| application.bundle_id.clone())
            .collect();
        if current != bundle_ids {
            self.geometry_poller.set_bundle_ids(bundle_ids.clone());
            let mut applications = AppVeilState::new(bundle_ids.clone()).applications;
            for application in &mut applications {
                if let Some(previous) = state
                    .applications
                    .iter_mut()
                    .find(|previous| previous.bundle_id == application.bundle_id)
                {
                    application.windows = std::mem::take(&mut previous.windows);
                }
            }
            state.applications = applications;
            self.snapshot.retain_bundle_ids(&bundle_ids);
            self.panels.retain(|window_id, _| {
                self.snapshot
                    .panels
                    .iter()
                    .any(|anchor| anchor.window_id == *window_id)
            });
        }
    }

    pub fn accepts_snapshot(&self, poller_id: u64, revision: u64) -> bool {
        self.geometry_poller.id == poller_id && self.geometry_poller.revision == revision
    }

    pub fn apply_snapshot(&mut self, mut snapshot: HostSnapshot) {
        let Some(main_thread_marker) = MainThreadMarker::new() else {
            log::error!("App Veil panels must be updated on the main thread");
            return;
        };
        self.panels.retain(|window_id, _| {
            snapshot
                .panels
                .iter()
                .any(|anchor| anchor.window_id == *window_id)
        });
        let desktop_height = CGDisplay::main().bounds().size.height;
        for anchor in &snapshot.panels {
            let rect = panel_rect(anchor.frame, desktop_height);
            let window_moved = self
                .snapshot
                .windows
                .iter()
                .find(|window| window.id == anchor.window_id)
                .zip(
                    snapshot
                        .windows
                        .iter()
                        .find(|window| window.id == anchor.window_id),
                )
                .is_some_and(|(previous, current)| {
                    previous.controller.frame != current.controller.frame
                });
            let panel = self.panels.entry(anchor.window_id).or_insert_with(|| {
                let state = self.state.clone();
                let event_loop_proxy = self.event_loop_proxy.clone();
                let window_id = anchor.window_id;
                let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                    NSPanel::alloc(main_thread_marker),
                    rect,
                    NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                    NSBackingStoreType::Buffered,
                    false,
                );
                unsafe { panel.setReleasedWhenClosed(false) };
                panel.setFloatingPanel(true);
                panel.setHidesOnDeactivate(false);
                panel.setBecomesKeyOnlyIfNeeded(true);
                panel.setIgnoresMouseEvents(false);
                panel.setHasShadow(false);
                panel.setLevel(NSFloatingWindowLevel);
                panel.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary,
                );
                panel.setOpaque(false);
                panel.setBackgroundColor(Some(&NSColor::clearColor()));
                let active = Rc::new(Cell::new(true));
                let callback_active = active.clone();
                let (content, button) = panel::content(
                    main_thread_marker,
                    Box::new(move || {
                        if !callback_active.get() {
                            return;
                        }
                        let mut state = state.lock().unwrap();
                        let Some(window) = state
                            .applications
                            .iter_mut()
                            .flat_map(|application| &mut application.windows)
                            .find(|window| window.window_id == window_id)
                        else {
                            return;
                        };
                        window.visible = !window.visible;
                        let _ = event_loop_proxy
                            .send_event(UserEvent::RefreshAppVeilFilter(state.clone()));
                    }),
                );
                panel.setContentView(Some(&content));
                panel.setSharingType(NSWindowSharingType::None);
                HostPanel {
                    window: panel,
                    button,
                    active,
                    last_moved_at: None,
                }
            });
            if window_moved || panel.window.frame() != rect {
                panel.last_moved_at = Some(Instant::now());
                if panel.window.isVisible() {
                    panel.window.orderOut(None);
                }
            }
            if panel.window.frame() != rect {
                panel.window.setFrame_display(rect, true);
            }
        }
        if let Some(applications) = snapshot.applications.take() {
            self.state.lock().unwrap().update_windows(applications);
        }
        snapshot.last_revealed_window_ids = if self.snapshot.windows == snapshot.windows {
            self.snapshot.last_revealed_window_ids.take()
        } else {
            None
        };
        self.snapshot = snapshot;
    }

    pub fn state(&self) -> AppVeilState {
        let mut state = self.state.lock().unwrap();
        state.control_panels_ids = self
            .panels
            .values()
            .map(|panel| panel.window.windowNumber() as u32)
            .collect();
        state.control_panels_ids.sort_unstable();
        state.clone()
    }

    pub fn update_visibility(&mut self) -> Option<AppVeilSnapshot> {
        let visible_window_ids: Vec<_> = self
            .state
            .lock()
            .unwrap()
            .applications
            .iter()
            .flat_map(|application| &application.windows)
            .filter(|window| window.visible)
            .map(|window| window.window_id)
            .collect();
        for (window_id, panel) in &self.panels {
            panel
                .button
                .update_appearance(visible_window_ids.contains(window_id));
            if panel_is_settled(panel.last_moved_at, Instant::now()) && !panel.window.isVisible() {
                panel.window.orderFrontRegardless();
            }
        }
        self.snapshot
            .changed_controller_snapshot(visible_window_ids)
    }
}

fn tracked_applications(bundle_ids: &[String]) -> Option<Vec<TrackedApplication>> {
    let descriptions = copy_window_info(kCGWindowListOptionAll, kCGNullWindowID)?;
    let mut applications = AppVeilState::new(bundle_ids.to_vec()).applications;
    let mut bundle_ids_by_pid = HashMap::new();
    for description in descriptions.get_all_values() {
        let description =
            unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(description as _) };
        let (Some(window_id), Some(owner_pid)) = (
            number(&description, unsafe { kCGWindowNumber }),
            number(&description, unsafe { kCGWindowOwnerPID }),
        ) else {
            continue;
        };
        let bundle_id = bundle_ids_by_pid.entry(owner_pid).or_insert_with(|| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(owner_pid)
                .and_then(|application| application.bundleIdentifier())
                .map(|bundle_id| bundle_id.to_string())
        });
        let Some(application) = applications
            .iter_mut()
            .find(|application| Some(&application.bundle_id) == bundle_id.as_ref())
        else {
            continue;
        };
        application.windows.push(TrackedWindow {
            window_id: window_id as u32,
            owner_pid,
            visible: false,
        });
    }
    for application in &mut applications {
        application
            .windows
            .sort_unstable_by_key(|window| window.window_id);
    }
    Some(applications)
}

fn geometry_enabled(bundle_ids: &[String]) -> bool {
    bundle_ids.iter().any(|bundle_id| {
        !matches!(
            bundle_id.as_str(),
            HOPP_BUNDLE_ID | NOTIFICATION_CENTER_BUNDLE_ID
        )
    })
}

fn has_running_protected_application(bundle_ids: &[String]) -> bool {
    bundle_ids.iter().any(|bundle_id| {
        if matches!(
            bundle_id.as_str(),
            HOPP_BUNDLE_ID | NOTIFICATION_CENTER_BUNDLE_ID
        ) {
            return false;
        }
        let bundle_id = objc2_foundation::NSString::from_str(bundle_id);
        !NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id).is_empty()
    })
}

#[cfg(test)]
fn snapshot(display_id: u32, bundle_ids: &[String]) -> Option<HostSnapshot> {
    snapshot_with_qualified_windows(display_id, bundle_ids, &mut HashMap::new())
}

fn retain_qualified_windows(
    qualified_windows: &mut HashMap<u32, i32>,
    applications: &[TrackedApplication],
) {
    qualified_windows.retain(|window_id, owner_pid| {
        applications.iter().any(|application| {
            application
                .windows
                .iter()
                .any(|window| window.window_id == *window_id && window.owner_pid == *owner_pid)
        })
    });
}

fn qualify_standard_window(
    qualified_windows: &mut HashMap<u32, i32>,
    window_id: u32,
    owner_pid: i32,
    ax_frame_matches: bool,
) -> bool {
    if ax_frame_matches {
        qualified_windows.insert(window_id, owner_pid);
    }
    qualified_windows.get(&window_id) == Some(&owner_pid)
}

fn snapshot_with_qualified_windows(
    display_id: u32,
    bundle_ids: &[String],
    qualified_windows: &mut HashMap<u32, i32>,
) -> Option<HostSnapshot> {
    if !geometry_enabled(bundle_ids) {
        return Some(HostSnapshot::default());
    }
    let display = CGDisplay::new(display_id).bounds();
    if display.size.width <= 0.0 || display.size.height <= 0.0 {
        return None;
    }
    let descriptions = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let applications = tracked_applications(bundle_ids);
    if let Some(applications) = &applications {
        retain_qualified_windows(qualified_windows, applications);
    }
    let protected: HashSet<&str> = bundle_ids.iter().map(String::as_str).collect();
    let mut application_frames = HashMap::new();
    let windows = descriptions
        .get_all_values()
        .into_iter()
        .filter_map(|description| {
            window_from_description(
                description,
                &protected,
                &mut application_frames,
                qualified_windows,
            )
        })
        .collect();
    let mut snapshot = snapshot_from_windows(windows, display_frame(display), bundle_ids);
    snapshot.applications = applications;
    Some(snapshot)
}

fn window_from_description(
    description: *const c_void,
    protected: &HashSet<&str>,
    application_frames: &mut HashMap<i32, Vec<Frame>>,
    qualified_windows: &mut HashMap<u32, i32>,
) -> Option<Window> {
    let description =
        unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(description as _) };
    if number(&description, unsafe { kCGWindowLayer })? != 0 {
        return None;
    }
    let id = number(&description, unsafe { kCGWindowNumber })? as u32;
    let process_id = number(&description, unsafe { kCGWindowOwnerPID })?;
    let bounds_key = unsafe { CFString::wrap_under_get_rule(kCGWindowBounds) };
    let bounds_value = description.find(&bounds_key)?;
    let bounds = unsafe { CFDictionary::wrap_under_get_rule(bounds_value.as_CFTypeRef() as _) };
    let frame = display_frame(CGRect::from_dict_representation(&bounds)?);
    let bundle_id = NSRunningApplication::runningApplicationWithProcessIdentifier(process_id)
        .and_then(|application| application.bundleIdentifier())
        .map(|bundle_id| bundle_id.to_string());
    if bundle_id.as_deref().is_some_and(|bundle_id| {
        protected.contains(bundle_id)
            && !matches!(bundle_id, HOPP_BUNDLE_ID | NOTIFICATION_CENTER_BUNDLE_ID)
    }) {
        let ax_frames = application_frames
            .entry(process_id)
            .or_insert_with(|| application_window_frames(process_id));
        let ax_frame_matches = ax_frames.contains(&frame);
        let qualified =
            qualify_standard_window(qualified_windows, id, process_id, ax_frame_matches);
        if !qualified {
            return None;
        }
    }

    Some(Window {
        id,
        frame,
        bundle_id,
    })
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(process_id: i32) -> CFTypeRef;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: core_foundation_sys::string::CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyMultipleAttributeValues(
        element: CFTypeRef,
        attributes: core_foundation_sys::array::CFArrayRef,
        options: u32,
        values: *mut core_foundation_sys::array::CFArrayRef,
    ) -> i32;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, result: *mut c_void) -> bool;
}

// shortcut: unavailable AX geometry omits host/controller rectangles; add another identity source if needed.
fn application_window_frames(process_id: i32) -> Vec<Frame> {
    let mut frames = Vec::new();
    unsafe {
        let application = AXUIElementCreateApplication(process_id);
        if application.is_null() {
            return frames;
        }
        let application = CFType::wrap_under_create_rule(application);
        if AXUIElementSetMessagingTimeout(application.as_CFTypeRef(), 0.1) != 0 {
            return frames;
        }
        let mut windows = std::ptr::null();
        let error = AXUIElementCopyAttributeValue(
            application.as_CFTypeRef(),
            CFString::new("AXWindows").as_concrete_TypeRef(),
            &mut windows,
        );
        if error != 0 || windows.is_null() {
            return frames;
        }
        let Some(windows) = CFType::wrap_under_create_rule(windows).downcast_into::<CFArray>()
        else {
            return frames;
        };
        let attributes = CFArray::from_CFTypes(&[
            CFString::new("AXSubrole"),
            CFString::new("AXPosition"),
            CFString::new("AXSize"),
        ]);
        for window in windows.get_all_values() {
            if CFGetTypeID(window) != AXUIElementGetTypeID()
                || AXUIElementSetMessagingTimeout(window, 0.1) != 0
            {
                continue;
            }
            let mut values = std::ptr::null();
            let error = AXUIElementCopyMultipleAttributeValues(
                window,
                attributes.as_concrete_TypeRef(),
                AX_COPY_STOP_ON_ERROR,
                &mut values,
            );
            if error == AX_ERROR_CANNOT_COMPLETE {
                break;
            }
            if error != 0 || values.is_null() {
                continue;
            }
            let values = CFArray::wrap_under_create_rule(values);
            if let Some(frame) = frame_from_ax_attributes(&values) {
                frames.push(frame);
            }
        }
    }
    frames
}

fn frame_from_ax_attributes(values: &CFArray) -> Option<Frame> {
    if values.len() != 3 {
        return None;
    }
    unsafe {
        let subrole = CFType::wrap_under_get_rule(*values.get(0)?).downcast_into::<CFString>()?;
        if subrole != "AXStandardWindow" {
            return None;
        }
        let position_value = *values.get(1)?;
        let size_value = *values.get(2)?;
        let mut position = CGPoint::new(0.0, 0.0);
        let mut size = CGSize::new(0.0, 0.0);
        if CFGetTypeID(position_value) == AXValueGetTypeID()
            && CFGetTypeID(size_value) == AXValueGetTypeID()
            && AXValueGetValue(
                position_value,
                AX_VALUE_CGPOINT,
                (&mut position as *mut CGPoint).cast(),
            )
            && AXValueGetValue(
                size_value,
                AX_VALUE_CGSIZE,
                (&mut size as *mut CGSize).cast(),
            )
        {
            return Some(display_frame(CGRect::new(&position, &size)));
        }
    }
    None
}

fn number(
    description: &CFDictionary<CFString, CFType>,
    key: core_foundation_sys::string::CFStringRef,
) -> Option<i32> {
    let key = unsafe { CFString::wrap_under_get_rule(key) };
    let value = description.find(&key)?;
    if !value.instance_of::<CFNumber>() {
        return None;
    }
    unsafe { CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as _) }.to_i32()
}

fn display_frame(rect: CGRect) -> Frame {
    Frame {
        origin_x: rect.origin.x,
        origin_y: rect.origin.y,
        extent: Extent {
            width: rect.size.width,
            height: rect.size.height,
        },
    }
}

fn clip(frame: Frame, display: Frame) -> Option<Frame> {
    let left = frame.origin_x.max(display.origin_x);
    let top = frame.origin_y.max(display.origin_y);
    let right = (frame.origin_x + frame.extent.width).min(display.origin_x + display.extent.width);
    let bottom =
        (frame.origin_y + frame.extent.height).min(display.origin_y + display.extent.height);
    (right > left && bottom > top).then_some(Frame {
        origin_x: left,
        origin_y: top,
        extent: Extent {
            width: right - left,
            height: bottom - top,
        },
    })
}

fn snapshot_from_windows(
    windows: Vec<Window>,
    display: Frame,
    protected_bundle_ids: &[String],
) -> HostSnapshot {
    let protected: HashSet<&str> = protected_bundle_ids.iter().map(String::as_str).collect();
    let mut window_bundle_ids: HashMap<u32, String> = windows
        .iter()
        .filter(|window| {
            window.bundle_id.as_deref().is_some_and(|bundle_id| {
                !matches!(bundle_id, HOPP_BUNDLE_ID | NOTIFICATION_CENTER_BUNDLE_ID)
                    && protected.contains(bundle_id)
            })
        })
        .map(|window| (window.id, window.bundle_id.clone().unwrap()))
        .collect();
    let windows: Vec<_> = windows
        .into_iter()
        .filter(|window| {
            window.bundle_id.as_deref() != Some(NOTIFICATION_CENTER_BUNDLE_ID)
                || !protected.contains(NOTIFICATION_CENTER_BUNDLE_ID)
        })
        .filter_map(|window| {
            Some(Window {
                frame: clip(window.frame, display)?,
                ..window
            })
        })
        .collect();
    let mut panels = Vec::new();
    let veils = visible_window_fragments(
        windows
            .iter()
            .map(|window| SelectableWindow {
                id: window.id,
                frame: window.frame,
            })
            .collect(),
    )
    .into_iter()
    .filter_map(|(window, fragments)| {
        let bundle_id = window_bundle_ids.remove(&window.id)?;
        if let Some(frame) = panel_anchor(window.frame, &fragments) {
            panels.push(PanelAnchor {
                window_id: window.id,
                frame,
            });
        }
        Some(HostWindow {
            id: window.id,
            bundle_id,
            controller: AppVeilWindow {
                frame: normalize(window.frame, display),
                visible_fragments: fragments
                    .into_iter()
                    .map(|fragment| normalize(fragment, display))
                    .collect(),
            },
        })
    })
    .collect();
    HostSnapshot {
        windows: veils,
        panels,
        applications: None,
        last_revealed_window_ids: None,
    }
}

fn panel_anchor(window: Frame, fragments: &[Frame]) -> Option<Frame> {
    use panel::{HEIGHT as PANEL_HEIGHT, INSET, WIDTH as PANEL_WIDTH};
    if window.extent.width < PANEL_WIDTH + 2.0 * INSET
        || window.extent.height < PANEL_HEIGHT + 2.0 * INSET
    {
        return None;
    }
    let anchor = Frame {
        origin_x: window.origin_x + window.extent.width - PANEL_WIDTH - INSET,
        origin_y: window.origin_y + window.extent.height - PANEL_HEIGHT - INSET,
        extent: Extent {
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
        },
    };
    let mut uncovered = vec![anchor];
    for fragment in fragments {
        let mut remaining = Vec::new();
        for frame in uncovered {
            subtract_frame(frame, *fragment, &mut remaining);
        }
        if remaining.is_empty() {
            return Some(anchor);
        }
        uncovered = remaining;
    }
    None
}

fn panel_rect(frame: Frame, desktop_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(
            frame.origin_x,
            desktop_height - frame.origin_y - frame.extent.height,
        ),
        NSSize::new(frame.extent.width, frame.extent.height),
    )
}

fn normalize(frame: Frame, display: Frame) -> NormalizedRect {
    NormalizedRect {
        x: ((frame.origin_x - display.origin_x) / display.extent.width).clamp(0.0, 1.0) as f32,
        y: ((frame.origin_y - display.origin_y) / display.extent.height).clamp(0.0, 1.0) as f32,
        width: (frame.extent.width / display.extent.width).clamp(0.0, 1.0) as f32,
        height: (frame.extent.height / display.extent.height).clamp(0.0, 1.0) as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    const CALCULATOR_BUNDLE_ID: &str = "com.apple.calculator";
    const TEXT_EDIT_BUNDLE_ID: &str = "com.apple.TextEdit";
    const CLOCK_PROCESS_CPUTIME_ID: u32 = 12;
    const CLOCK_THREAD_CPUTIME_ID: u32 = 16;

    #[test]
    fn panel_stays_hidden_until_movement_has_settled() {
        let started_at = Instant::now();
        assert!(panel_is_settled(None, started_at));
        assert!(!panel_is_settled(Some(started_at), started_at));
        assert!(!panel_is_settled(
            Some(started_at),
            started_at + Duration::from_millis(199),
        ));
        assert!(panel_is_settled(
            Some(started_at),
            started_at + PANEL_SETTLE_DELAY
        ));
        let moved_again_at = started_at + Duration::from_millis(150);
        assert!(!panel_is_settled(
            Some(moved_again_at),
            started_at + PANEL_SETTLE_DELAY
        ));
        assert!(panel_is_settled(
            Some(moved_again_at),
            moved_again_at + PANEL_SETTLE_DELAY,
        ));
    }

    extern "C" {
        fn clock_gettime_nsec_np(clock_id: u32) -> u64;
        fn AXValueCreate(value_type: u32, value: *const c_void) -> CFTypeRef;
    }

    // Native CPU clocks exclude sleeping; 100% means one fully occupied CPU core.
    fn cpu_usage_percent(before: u64, after: u64, elapsed: Duration) -> Option<f64> {
        if before == 0 || after == 0 || elapsed.is_zero() {
            return None;
        }
        let used = after.checked_sub(before)?;
        Some(100.0 * used as f64 / elapsed.as_nanos() as f64)
    }

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            origin_x: x,
            origin_y: y,
            extent: Extent { width, height },
        }
    }

    #[test]
    fn qualified_window_identity_survives_movement_but_not_owner_or_inventory_changes() {
        let mut qualified_windows = HashMap::new();
        assert!(!qualify_standard_window(
            &mut qualified_windows,
            7,
            42,
            false
        ));
        assert!(qualify_standard_window(&mut qualified_windows, 7, 42, true));
        assert!(qualify_standard_window(
            &mut qualified_windows,
            7,
            42,
            false
        ));
        assert!(!qualify_standard_window(
            &mut qualified_windows,
            8,
            42,
            false
        ));
        assert!(!qualify_standard_window(
            &mut qualified_windows,
            7,
            43,
            false
        ));

        let mut applications = vec![TrackedApplication {
            bundle_id: "protected".into(),
            windows: vec![TrackedWindow {
                window_id: 7,
                owner_pid: 42,
                visible: false,
            }],
        }];
        // The all-window inventory retains minimized windows even without an on-screen anchor.
        retain_qualified_windows(&mut qualified_windows, &applications);
        assert!(qualify_standard_window(
            &mut qualified_windows,
            7,
            42,
            false
        ));

        applications[0].windows[0].owner_pid = 43;
        retain_qualified_windows(&mut qualified_windows, &applications);
        assert!(qualified_windows.is_empty());
        assert!(!qualify_standard_window(
            &mut qualified_windows,
            7,
            43,
            false
        ));

        assert!(qualify_standard_window(&mut qualified_windows, 7, 43, true));
        applications[0].windows.clear();
        retain_qualified_windows(&mut qualified_windows, &applications);
        assert!(qualified_windows.is_empty());

        assert!(qualify_standard_window(&mut qualified_windows, 7, 43, true));
        retain_qualified_windows(&mut qualified_windows, &[]);
        assert!(qualified_windows.is_empty());
    }

    fn ax_attributes(subrole: &str, frame: Frame) -> CFArray {
        let position = CGPoint::new(frame.origin_x, frame.origin_y);
        let size = CGSize::new(frame.extent.width, frame.extent.height);
        unsafe {
            let position = AXValueCreate(AX_VALUE_CGPOINT, (&position as *const CGPoint).cast());
            let size = AXValueCreate(AX_VALUE_CGSIZE, (&size as *const CGSize).cast());
            assert!(!position.is_null() && !size.is_null());
            CFArray::from_CFTypes(&[
                CFString::new(subrole).as_CFType(),
                CFType::wrap_under_create_rule(position),
                CFType::wrap_under_create_rule(size),
            ])
            .to_untyped()
        }
    }

    #[test]
    fn ax_attributes_require_standard_subrole_and_valid_geometry_types() {
        let expected = frame(-500.0, 252.0, 1172.0, 948.0);
        let attributes = ax_attributes("AXStandardWindow", expected);
        assert_eq!(frame_from_ax_attributes(&attributes), Some(expected));
        assert_eq!(
            frame_from_ax_attributes(&ax_attributes("AXDialog", expected)),
            None,
        );
        let values: Vec<_> = attributes
            .get_all_values()
            .into_iter()
            .map(|value| unsafe { CFType::wrap_under_get_rule(value) })
            .collect();
        assert_eq!(
            frame_from_ax_attributes(&CFArray::from_CFTypes(&values[..2]).to_untyped()),
            None,
        );
        for index in 0..3 {
            let mut malformed = values.clone();
            malformed[index] = CFNumber::from(1_i32).as_CFType();
            assert_eq!(
                frame_from_ax_attributes(&CFArray::from_CFTypes(&malformed).to_untyped()),
                None,
            );
        }
        let swapped =
            CFArray::from_CFTypes(&[values[0].clone(), values[2].clone(), values[1].clone()]);
        assert_eq!(frame_from_ax_attributes(&swapped.to_untyped()), None);
    }

    #[test]
    fn cpu_usage_uses_elapsed_time_and_rejects_unavailable_clocks() {
        let elapsed = Duration::from_secs(5);
        assert_eq!(cpu_usage_percent(1, 250_000_001, elapsed), Some(5.0));
        assert_eq!(cpu_usage_percent(1, 7_500_000_001, elapsed), Some(150.0));
        assert_eq!(cpu_usage_percent(1, 1, elapsed), Some(0.0));
        assert_eq!(cpu_usage_percent(0, 1, elapsed), None);
        assert_eq!(cpu_usage_percent(1, 0, elapsed), None);
        assert_eq!(cpu_usage_percent(2, 1, elapsed), None);
        assert_eq!(cpu_usage_percent(1, 2, Duration::ZERO), None);
    }

    #[test]
    fn native_cpu_clocks_count_work_and_exclude_thread_sleep() {
        let worker_before = unsafe { clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) };
        let process_before = unsafe { clock_gettime_nsec_np(CLOCK_PROCESS_CPUTIME_ID) };
        let sleep_started_at = Instant::now();
        thread::sleep(Duration::from_millis(25));
        let elapsed = sleep_started_at.elapsed();
        let worker_after_sleep = unsafe { clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) };
        assert!(cpu_usage_percent(worker_before, worker_after_sleep, elapsed).unwrap() < 50.0);

        let work_started_at = Instant::now();
        while work_started_at.elapsed() < Duration::from_millis(3) {
            std::hint::black_box(work_started_at);
        }
        let worker_after_work = unsafe { clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) };
        let process_after = unsafe { clock_gettime_nsec_np(CLOCK_PROCESS_CPUTIME_ID) };
        assert!(worker_after_work > worker_after_sleep);
        assert!(process_before > 0 && process_after > process_before);
    }

    #[test]
    #[ignore = "benchmarks native polling; requires visible Bear and macOS screen/Accessibility permissions"]
    fn benchmark_native_polling_cpu() {
        let _ = env_logger::Builder::new()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .try_init();
        let bundle_ids = vec!["net.shinyfrog.bear".to_string()];
        let display_id = CGDisplay::main().id;
        let initial = autoreleasepool(|_| snapshot(display_id, &bundle_ids))
            .expect("WindowServer snapshot unavailable; grant screen-recording access");
        assert!(
            !initial.panels.is_empty(),
            "Bear must be visible and its AX window geometry accessible",
        );
        log::info!(
            "App Veil benchmark scene: display_id={display_id} protected_bundle={} veils={} panels={}",
            bundle_ids[0],
            initial.windows.len(),
            initial.panels.len(),
        );

        for interval_ms in [50, 100, 250, 1000] {
            let interval = Duration::from_millis(interval_ms);
            let (sender, receiver) = mpsc::channel();
            let mut poller = GeometryPoller::spawn(
                NEXT_GEOMETRY_POLLER_ID.fetch_add(1, Ordering::Relaxed),
                display_id,
                bundle_ids.clone(),
                interval,
                move |_, display_id, bundle_ids| {
                    let started_at = Instant::now();
                    let cpu_before = unsafe { clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) };
                    let snapshot = autoreleasepool(|_| {
                        has_running_protected_application(bundle_ids)
                            .then(|| snapshot(display_id, bundle_ids))
                            .flatten()
                    });
                    let elapsed = started_at.elapsed();
                    let counts =
                        snapshot.map(|snapshot| (snapshot.windows.len(), snapshot.panels.len()));
                    let _ = sender.send((started_at, cpu_before, elapsed, counts));
                },
            );
            let timeout = interval + Duration::from_secs(3);
            receiver.recv_timeout(timeout).expect("warmup poll missing");
            let (sample_started_at, cpu_before, mut previous_poll_time, mut previous_counts) =
                receiver
                    .recv_timeout(timeout)
                    .expect("first sample missing");
            let mut polls = 0_u64;
            let mut total_poll_time = Duration::ZERO;
            let mut max_poll_time = Duration::ZERO;
            loop {
                let (started_at, cpu_after, poll_time, counts) = receiver
                    .recv_timeout(timeout)
                    .expect("benchmark poll missing");
                let (veils, panels) =
                    previous_counts.expect("WindowServer snapshot failed during benchmark");
                assert!(
                    veils > 0 && panels > 0,
                    "Bear/AX geometry disappeared during benchmark",
                );
                polls += 1;
                total_poll_time += previous_poll_time;
                max_poll_time = max_poll_time.max(previous_poll_time);
                let elapsed = started_at.duration_since(sample_started_at);
                if elapsed >= Duration::from_secs(6) {
                    let cpu_percent = cpu_usage_percent(cpu_before, cpu_after, elapsed)
                        .expect("native worker CPU clock unavailable");
                    let cpu_ms = (cpu_after - cpu_before) as f64 / 1_000_000.0;
                    log::info!(
                        "App Veil benchmark: interval_ms={interval_ms} sample_s={:.3} polls={polls} \
                         polls_per_s={:.2} worker_cpu_ms={cpu_ms:.3} worker_cpu_percent={cpu_percent:.3} \
                         avg_cpu_ms_per_poll={:.3} avg_poll_ms={:.3} max_poll_ms={:.3}",
                        elapsed.as_secs_f64(),
                        polls as f64 / elapsed.as_secs_f64(),
                        cpu_ms / polls as f64,
                        total_poll_time.as_secs_f64() * 1000.0 / polls as f64,
                        max_poll_time.as_secs_f64() * 1000.0,
                    );
                    break;
                }
                previous_poll_time = poll_time;
                previous_counts = counts;
            }
            let _ = poller.sender.send(GeometryPollerMessage::Stop);
            poller
                .thread
                .take()
                .unwrap()
                .join()
                .expect("benchmark worker panicked");
        }
    }

    #[test]
    fn panel_follows_visible_anchor_and_converts_global_coordinates() {
        let display = frame(1000.0, -500.0, 808.0, 600.0);
        let protected_window = frame(1200.0, -300.0, 208.0, 200.0);
        let snapshot_at = |window_frame, occluders: Vec<Window>| {
            let mut windows = occluders;
            windows.push(Window {
                id: 7,
                frame: window_frame,
                bundle_id: Some("protected".into()),
            });
            snapshot_from_windows(windows, display, &["protected".into()])
        };
        let initial = snapshot_at(protected_window, Vec::new());
        assert_eq!(initial.panels.len(), 1);
        assert_eq!(initial.panels[0].window_id, 7);
        let anchor = frame(1212.0, -148.0, 184.0, 36.0);
        assert_eq!(initial.panels[0].frame, anchor);
        assert_eq!(
            panel_rect(anchor, 900.0),
            NSRect::new(NSPoint::new(1212.0, 1012.0), NSSize::new(184.0, 36.0)),
        );
        let moved = snapshot_at(frame(1240.0, -280.0, 240.0, 200.0), Vec::new());
        assert_eq!(moved.panels[0].frame, frame(1284.0, -128.0, 184.0, 36.0));
        let covered_anchor = snapshot_at(
            protected_window,
            vec![Window {
                id: 8,
                frame: anchor,
                bundle_id: Some("foreground".into()),
            }],
        );
        assert!(covered_anchor.panels.is_empty());
        assert_eq!(covered_anchor.windows.len(), 1);
        let split_fragments = snapshot_at(
            protected_window,
            vec![Window {
                id: 8,
                frame: frame(1200.0, -290.0, 50.0, 10.0),
                bundle_id: Some("foreground".into()),
            }],
        );
        assert_eq!(split_fragments.panels[0].frame, anchor);
        let off_display_edge = snapshot_at(frame(1600.0, -300.0, 300.0, 200.0), Vec::new());
        assert_eq!(
            off_display_edge.panels[0].frame,
            frame(1612.0, -148.0, 184.0, 36.0),
        );
        assert_eq!(off_display_edge.windows.len(), 1);
        let off_display_top = snapshot_at(frame(1200.0, -600.0, 208.0, 200.0), Vec::new());
        assert_eq!(
            off_display_top.panels[0].frame,
            frame(1212.0, -448.0, 184.0, 36.0),
        );
        assert!(snapshot_at(frame(0.0, 0.0, 208.0, 200.0), Vec::new())
            .panels
            .is_empty());
        assert!(panel_anchor(frame(0.0, 0.0, 20.0, 20.0), &[display]).is_none());
    }

    #[test]
    fn selection_dialog_is_excluded_from_panels_and_controller_rectangles() {
        let display = frame(0.0, 0.0, 1500.0, 1200.0);
        let document = frame(98.0, 162.0, 1172.0, 948.0);
        let dialog = frame(500.0, 252.0, 53.0, 48.0);
        let other_document = frame(1280.0, 0.0, 208.0, 120.0);
        let ax_frames: Vec<_> = [
            ax_attributes("AXDialog", dialog),
            ax_attributes("AXStandardWindow", document),
            ax_attributes("AXStandardWindow", other_document),
        ]
        .iter()
        .filter_map(frame_from_ax_attributes)
        .collect();
        assert!(!ax_frames.contains(&dialog));
        assert!(ax_frames.contains(&document));
        assert!(ax_frames.contains(&other_document));
        let windows = [(2659, dialog), (2081, document), (2082, other_document)]
            .into_iter()
            .filter(|(_, frame)| ax_frames.contains(frame))
            .map(|(id, frame)| Window {
                id,
                frame,
                bundle_id: Some("protected".into()),
            })
            .collect();
        let snapshot = snapshot_from_windows(windows, display, &["protected".into()]);
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(
            snapshot.controller_snapshot(&[]).windows[0].frame,
            normalize(document, display),
        );
        assert_eq!(
            snapshot.controller_snapshot(&[]).windows[0].visible_fragments,
            vec![normalize(document, display)],
        );
        assert_eq!(
            snapshot
                .panels
                .iter()
                .map(|panel| panel.window_id)
                .collect::<Vec<_>>(),
            vec![2081, 2082],
        );
    }

    #[test]
    fn revealing_one_window_preserves_panels_and_underlying_occlusion() {
        let mut snapshot = snapshot_from_windows(
            vec![
                Window {
                    id: 1,
                    frame: frame(100.0, 100.0, 300.0, 200.0),
                    bundle_id: Some("protected".into()),
                },
                Window {
                    id: 2,
                    frame: frame(0.0, 0.0, 700.0, 600.0),
                    bundle_id: Some("protected".into()),
                },
            ],
            frame(0.0, 0.0, 1000.0, 800.0),
            &["protected".into()],
        );
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.panels.len(), 2);
        let hidden = snapshot.controller_snapshot(&[]);
        let revealed = snapshot.controller_snapshot(&[1]);
        assert_eq!(revealed.windows, vec![hidden.windows[1].clone()]);
        assert!(revealed.windows[0].visible_fragments.len() > 1);
        assert!(snapshot.controller_snapshot(&[1, 2]).windows.is_empty());
        assert_eq!(snapshot.controller_snapshot(&[]), hidden);
        assert_eq!(
            snapshot.changed_controller_snapshot(vec![]),
            Some(hidden.clone())
        );
        assert!(snapshot.changed_controller_snapshot(vec![]).is_none());
        assert_eq!(
            snapshot.changed_controller_snapshot(vec![1]),
            Some(revealed)
        );
        assert!(snapshot.changed_controller_snapshot(vec![1]).is_none());
        assert_eq!(snapshot.changed_controller_snapshot(vec![]), Some(hidden));
        assert_eq!(
            snapshot.changed_controller_snapshot(vec![1, 2]),
            Some(AppVeilSnapshot::default()),
        );
        assert!(snapshot.changed_controller_snapshot(vec![1, 2]).is_none());
    }

    #[test]
    fn disabling_one_app_preserves_other_apps_panels_and_veils() {
        let mut snapshot = snapshot_from_windows(
            vec![
                Window {
                    id: 1,
                    frame: frame(0.0, 0.0, 300.0, 300.0),
                    bundle_id: Some("first".into()),
                },
                Window {
                    id: 2,
                    frame: frame(400.0, 0.0, 300.0, 300.0),
                    bundle_id: Some("second".into()),
                },
            ],
            frame(0.0, 0.0, 1000.0, 800.0),
            &["first".into(), "second".into()],
        );
        let remaining_veil = snapshot.controller_snapshot(&[]).windows[1].clone();
        assert!(snapshot.changed_controller_snapshot(vec![]).is_some());
        assert!(snapshot.changed_controller_snapshot(vec![]).is_none());
        snapshot.retain_bundle_ids(&["second".into()]);
        assert!(snapshot.changed_controller_snapshot(vec![]).is_some());
        assert!(snapshot.changed_controller_snapshot(vec![]).is_none());
        assert_eq!(snapshot.windows[0].id, 2);
        assert_eq!(
            snapshot.controller_snapshot(&[]).windows,
            vec![remaining_veil]
        );
        assert_eq!(snapshot.panels.len(), 1);
        assert_eq!(snapshot.panels[0].window_id, 2);
        snapshot.retain_bundle_ids(&[]);
        assert!(snapshot.windows.is_empty());
        assert!(snapshot.panels.is_empty());
        assert_eq!(
            snapshot.changed_controller_snapshot(vec![]),
            Some(AppVeilSnapshot::default())
        );
        assert!(snapshot.changed_controller_snapshot(vec![]).is_none());
    }

    #[test]
    fn empty_controller_snapshot_is_published_once() {
        let mut snapshot = HostSnapshot::default();
        assert_eq!(
            snapshot.changed_controller_snapshot(vec![]),
            Some(AppVeilSnapshot::default())
        );
        assert!(snapshot.changed_controller_snapshot(vec![]).is_none());
    }

    #[test]
    fn geometry_poller_runs_off_thread_and_accepts_bundle_updates() {
        let (callback_sender, callback_receiver) = mpsc::channel();
        let mut poller = GeometryPoller::spawn(
            1,
            7,
            vec!["first".into()],
            Duration::from_millis(10),
            move |revision, display_id, bundle_ids| {
                let _ = callback_sender.send((revision, display_id, bundle_ids.to_vec()));
            },
        );

        assert_eq!(
            callback_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("initial geometry poll was not delivered"),
            (0, 7, vec!["first".to_string()])
        );
        assert_eq!(
            callback_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("periodic geometry poll was not delivered"),
            (0, 7, vec!["first".to_string()])
        );

        poller.set_bundle_ids(vec!["second".into()]);
        let updated =
            std::iter::from_fn(|| callback_receiver.recv_timeout(Duration::from_secs(1)).ok())
                .find(|(_, _, bundle_ids)| bundle_ids == &["second"]);
        assert_eq!(updated, Some((1, 7, vec!["second".to_string()])));
        drop(poller);
        while callback_receiver.try_recv().is_ok() {}
        assert!(callback_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_err());
    }

    #[test]
    fn emits_only_visible_protected_fragments_when_partly_occluded() {
        let display = frame(100.0, 50.0, 1000.0, 500.0);
        let snapshot = snapshot_from_windows(
            vec![
                Window {
                    id: 1,
                    frame: frame(250.0, 100.0, 100.0, 100.0),
                    bundle_id: Some("foreground".into()),
                },
                Window {
                    id: 2,
                    frame: frame(50.0, 0.0, 300.0, 200.0),
                    bundle_id: Some("protected".into()),
                },
            ],
            display,
            &["protected".into()],
        );

        assert_eq!(
            snapshot.controller_snapshot(&[]).windows,
            vec![AppVeilWindow {
                frame: NormalizedRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.25,
                    height: 0.3,
                },
                visible_fragments: vec![
                    NormalizedRect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.25,
                        height: 0.1,
                    },
                    NormalizedRect {
                        x: 0.0,
                        y: 0.1,
                        width: 0.15,
                        height: 0.2,
                    },
                ],
            }]
        );
    }

    #[test]
    fn omits_fully_occluded_protected_window() {
        let display = frame(0.0, 0.0, 100.0, 100.0);
        let snapshot = snapshot_from_windows(
            vec![
                Window {
                    id: 1,
                    frame: display,
                    bundle_id: Some("foreground".into()),
                },
                Window {
                    id: 2,
                    frame: frame(10.0, 10.0, 20.0, 20.0),
                    bundle_id: Some("protected".into()),
                },
            ],
            display,
            &["protected".into()],
        );

        assert!(snapshot.windows.is_empty());
    }

    #[test]
    fn captured_hopp_window_occludes_without_getting_a_veil() {
        let display = frame(0.0, 0.0, 100.0, 100.0);
        let snapshot = snapshot_from_windows(
            vec![
                Window {
                    id: 1,
                    frame: display,
                    bundle_id: Some(HOPP_BUNDLE_ID.into()),
                },
                Window {
                    id: 2,
                    frame: display,
                    bundle_id: Some("protected".into()),
                },
            ],
            display,
            &["protected".into(), HOPP_BUNDLE_ID.into()],
        );

        assert!(snapshot.windows.is_empty());
    }

    #[test]
    fn notification_center_occludes_only_when_captured() {
        let display = frame(0.0, 0.0, 100.0, 100.0);
        let windows = || {
            vec![
                Window {
                    id: 1,
                    frame: display,
                    bundle_id: Some(NOTIFICATION_CENTER_BUNDLE_ID.into()),
                },
                Window {
                    id: 2,
                    frame: display,
                    bundle_id: Some("protected".into()),
                },
            ]
        };

        assert!(
            snapshot_from_windows(windows(), display, &["protected".into()])
                .controller_snapshot(&[])
                .windows
                .is_empty()
        );
        assert_eq!(
            snapshot_from_windows(
                windows(),
                display,
                &["protected".into(), NOTIFICATION_CENTER_BUNDLE_ID.into()],
            )
            .controller_snapshot(&[])
            .windows
            .len(),
            1
        );
    }

    #[test]
    fn clips_and_repositions_window_returning_to_selected_display() {
        let display = frame(1440.0, 0.0, 1000.0, 800.0);
        let bundle_ids = ["protected".into()];
        let snapshot_at = |window_frame| {
            snapshot_from_windows(
                vec![Window {
                    id: 1,
                    frame: window_frame,
                    bundle_id: Some("protected".into()),
                }],
                display,
                &bundle_ids,
            )
        };

        assert!(snapshot_at(frame(200.0, 100.0, 400.0, 400.0))
            .controller_snapshot(&[])
            .windows
            .is_empty());
        assert_eq!(
            snapshot_at(frame(1340.0, 100.0, 200.0, 400.0))
                .controller_snapshot(&[])
                .windows,
            vec![AppVeilWindow {
                frame: NormalizedRect {
                    x: 0.0,
                    y: 0.125,
                    width: 0.1,
                    height: 0.5,
                },
                visible_fragments: vec![NormalizedRect {
                    x: 0.0,
                    y: 0.125,
                    width: 0.1,
                    height: 0.5,
                }],
            }]
        );
        assert_eq!(
            snapshot_at(frame(1940.0, 100.0, 200.0, 400.0))
                .controller_snapshot(&[])
                .windows,
            vec![AppVeilWindow {
                frame: NormalizedRect {
                    x: 0.5,
                    y: 0.125,
                    width: 0.2,
                    height: 0.5,
                },
                visible_fragments: vec![NormalizedRect {
                    x: 0.5,
                    y: 0.125,
                    width: 0.2,
                    height: 0.5,
                }],
            }]
        );
    }

    struct ApplicationGuard(i32);

    impl Drop for ApplicationGuard {
        fn drop(&mut self) {
            if let Some(application) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(self.0)
            {
                application.terminate();
            }
        }
    }

    #[test]
    #[ignore = "opens Calculator and requires macOS Accessibility permission"]
    fn calculator_window_lifecycle_updates_snapshot_geometry() {
        let existing = calculator_process_ids();
        assert!(Command::new("open")
            .args(["-na", "/System/Applications/Calculator.app"])
            .status()
            .is_ok_and(|status| status.success()));
        let process_id = wait_until(|| {
            calculator_process_ids()
                .difference(&existing)
                .next()
                .copied()
        })
        .expect("Calculator did not launch as a distinct process");
        let guard = ApplicationGuard(process_id);
        let display_id = CGDisplay::main().id;
        let main = CGDisplay::new(display_id).bounds();
        let bundle_ids = vec![CALCULATOR_BUNDLE_ID.to_string()];
        wait_until(|| (system_events_window_count(process_id)? > 0).then_some(()))
            .expect("Calculator did not create a window");
        run_system_events(
            process_id,
            &format!(
                "set position of first window to {{{}, {}}}",
                main.origin.x + 160.0,
                main.origin.y + 180.0
            ),
        );
        let before = wait_until(|| {
            snapshot(display_id, &bundle_ids).filter(|snapshot| !snapshot.windows.is_empty())
        })
        .expect("Calculator window did not appear in the App Veil snapshot");

        run_system_events(process_id, "set position of first window to {260, 280}");
        run_system_events(process_id, "set size of first window to {380, 440}");
        wait_until(|| {
            let current = snapshot(display_id, &bundle_ids)?;
            (current.controller_snapshot(&[]) != before.controller_snapshot(&[])).then_some(())
        })
        .expect("move/resize did not update App Veil geometry");

        if let Some(other_display_id) = CGDisplay::active_displays()
            .ok()
            .and_then(|displays| displays.into_iter().find(|id| *id != display_id))
        {
            let other = CGDisplay::new(other_display_id).bounds();
            run_system_events(
                process_id,
                &format!(
                    "set position of first window to {{{}, {}}}",
                    other.origin.x + 100.0,
                    other.origin.y + 100.0
                ),
            );
            wait_until(|| {
                snapshot(display_id, &bundle_ids)?
                    .controller_snapshot(&[])
                    .windows
                    .is_empty()
                    .then_some(())
            })
            .expect("moving Calculator to another display did not remove its veil");

            run_system_events(
                process_id,
                &format!(
                    "set position of first window to {{{}, {}}}",
                    main.origin.x + 100.0,
                    main.origin.y + 100.0
                ),
            );
            let returned = wait_until(|| {
                snapshot(display_id, &bundle_ids)?
                    .controller_snapshot(&[])
                    .windows
                    .into_iter()
                    .next()
            })
            .expect("returning Calculator to the selected display did not restore its veil");

            run_system_events(
                process_id,
                &format!(
                    "set position of first window to {{{}, {}}}",
                    main.origin.x + main.size.width * 0.5,
                    main.origin.y + 100.0
                ),
            );
            wait_until(|| {
                let moved = snapshot(display_id, &bundle_ids)?
                    .controller_snapshot(&[])
                    .windows
                    .into_iter()
                    .next()?;
                (moved.frame.x > returned.frame.x).then_some(())
            })
            .expect("moving Calculator right positioned its veil on the wrong side");
        }

        run_system_events(
            process_id,
            "set value of attribute \"AXMinimized\" of first window to true",
        );
        wait_until(|| {
            let current = snapshot(display_id, &bundle_ids)?;
            (current.windows.len() < before.windows.len()).then_some(())
        })
        .expect("minimize did not remove the Calculator veil");

        run_system_events(
            process_id,
            "set value of attribute \"AXMinimized\" of first window to false",
        );
        wait_until(|| {
            let current = snapshot(display_id, &bundle_ids)?;
            (current.windows.len() >= before.windows.len()).then_some(())
        })
        .expect("restore did not return the Calculator veil");

        drop(guard);
        wait_until(|| {
            let current = snapshot(display_id, &bundle_ids)?;
            (current.windows.len() < before.windows.len()).then_some(())
        })
        .expect("close did not remove the Calculator veil");
    }

    #[test]
    #[ignore = "opens TextEdit and requires macOS Accessibility permission"]
    fn text_edit_new_window_close_updates_snapshot_geometry() {
        let existing = application_process_ids(TEXT_EDIT_BUNDLE_ID);
        assert!(Command::new("open")
            .args(["-na", "/System/Applications/TextEdit.app"])
            .status()
            .is_ok_and(|status| status.success()));
        let process_id = wait_until(|| {
            application_process_ids(TEXT_EDIT_BUNDLE_ID)
                .difference(&existing)
                .next()
                .copied()
        })
        .expect("TextEdit did not launch as a distinct process");
        let guard = ApplicationGuard(process_id);
        let display_id = CGDisplay::main().id;
        let display = CGDisplay::new(display_id).bounds();
        let bundle_ids = vec![TEXT_EDIT_BUNDLE_ID.to_string()];

        run_system_events(process_id, "set frontmost to true");
        run_system_events(process_id, "keystroke \"n\" using command down");
        let initial_window_count = wait_until(|| {
            let count = system_events_window_count(process_id)?;
            (count > 0).then_some(count)
        })
        .expect("TextEdit did not create its initial window");
        run_system_events(
            process_id,
            &format!(
                "set position of first window to {{{}, {}}}",
                display.origin.x + 100.0,
                display.origin.y + 100.0
            ),
        );
        let initial_veil_count = wait_until(|| {
            let count = snapshot(display_id, &bundle_ids)?.windows.len();
            (count > 0).then_some(count)
        })
        .expect("TextEdit window did not appear in the App Veil snapshot");

        run_system_events(process_id, "keystroke \"n\" using command down");
        wait_until(|| {
            (system_events_window_count(process_id)? > initial_window_count).then_some(())
        })
        .expect("TextEdit did not create a second window");
        run_system_events(
            process_id,
            &format!(
                "set position of first window to {{{}, {}}}",
                display.origin.x + display.size.width * 0.65,
                display.origin.y + 100.0
            ),
        );
        let windows_with_new = wait_until(|| {
            let count = snapshot(display_id, &bundle_ids)?.windows.len();
            (count > initial_veil_count).then_some(count)
        })
        .expect("new TextEdit window did not add an App Veil rectangle");

        run_system_events(process_id, "keystroke \"w\" using command down");
        wait_until(|| {
            let count = snapshot(display_id, &bundle_ids)?.windows.len();
            (count < windows_with_new).then_some(())
        })
        .expect("closing the new TextEdit window did not remove its veil");
        drop(guard);
        wait_until(|| {
            (!application_process_ids(TEXT_EDIT_BUNDLE_ID).contains(&process_id)).then_some(())
        })
        .expect("temporary TextEdit process did not terminate");
    }

    fn calculator_process_ids() -> HashSet<i32> {
        application_process_ids(CALCULATOR_BUNDLE_ID)
    }

    fn application_process_ids(bundle_id: &str) -> HashSet<i32> {
        let bundle_id = objc2_foundation::NSString::from_str(bundle_id);
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id)
            .into_iter()
            .map(|application| application.processIdentifier())
            .collect()
    }

    fn run_system_events(process_id: i32, command: &str) {
        let script = format!(
            "tell application \"System Events\" to tell first process whose unix id is {process_id} to {command}"
        );
        assert!(Command::new("osascript")
            .args(["-e", &script])
            .status()
            .is_ok_and(|status| status.success()));
    }

    fn system_events_window_count(process_id: i32) -> Option<usize> {
        let script = format!(
            "tell application \"System Events\" to tell first process whose unix id is {process_id} to count windows"
        );
        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }

    fn wait_until<T>(mut operation: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(value) = operation() {
                return Some(value);
            }
            thread::sleep(Duration::from_millis(100));
        }
        None
    }
}
