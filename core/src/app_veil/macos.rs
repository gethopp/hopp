use std::{
    collections::HashSet,
    ffi::c_void,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};

use core_foundation::{
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_graphics::{
    display::CGDisplay,
    geometry::CGRect,
    window::{
        copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowNumber,
        kCGWindowOwnerPID,
    },
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSRunningApplication;
use winit::event_loop::EventLoopProxy;

use crate::{
    capture::running_applications_observer::RunningApplicationsObserver,
    room_service::{AppVeilSnapshot, AppVeilWindow, NormalizedRect},
    utils::geometry::{Extent, Frame},
    visible_window_fragments, SelectableWindow, UserEvent,
};

const HOPP_BUNDLE_ID: &str = "com.hopp.app";
const NOTIFICATION_CENTER_BUNDLE_ID: &str = "com.apple.notificationcenterui";
const GEOMETRY_POLL_INTERVAL: Duration = Duration::from_secs(1);
static NEXT_GEOMETRY_POLLER_ID: AtomicU64 = AtomicU64::new(1);

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
                        if is_active {
                            log::info!(
                                "App Veil geometry poller resumed: a protected application process is running"
                            );
                        } else {
                            log::info!(
                                "App Veil geometry poller idle: no protected application processes are running"
                            );
                        }
                        geometry_active = Some(is_active);
                    }
                    if !is_active {
                        if state_changed {
                            let _ = event_loop_proxy.send_event(UserEvent::PolledAppVeilSnapshot(
                                id,
                                revision,
                                AppVeilSnapshot::default(),
                            ));
                        }
                        return;
                    }
                    if let Some(snapshot) = snapshot(display_id, bundle_ids) {
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
    bundle_ids: Vec<String>,
    observed_bundle_ids: Arc<Mutex<HashSet<String>>>,
    geometry_poller: GeometryPoller,
    _running_applications_observer: RunningApplicationsObserver,
}

impl AppVeilHost {
    pub fn new(
        display_id: u32,
        bundle_ids: Vec<String>,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Option<Self> {
        let observed_bundle_ids = Arc::new(Mutex::new(
            bundle_ids.iter().cloned().collect::<HashSet<_>>(),
        ));
        let running_applications_observer = RunningApplicationsObserver::new(
            event_loop_proxy.clone(),
            observed_bundle_ids.clone(),
        )?;
        let geometry_poller = GeometryPoller::new(display_id, bundle_ids.clone(), event_loop_proxy);
        Some(Self {
            bundle_ids,
            observed_bundle_ids,
            geometry_poller,
            _running_applications_observer: running_applications_observer,
        })
    }

    pub fn set_bundle_ids(&mut self, bundle_ids: Vec<String>) {
        if self.bundle_ids != bundle_ids {
            self.geometry_poller.set_bundle_ids(bundle_ids.clone());
            *self.observed_bundle_ids.lock().unwrap() = bundle_ids.iter().cloned().collect();
            self.bundle_ids = bundle_ids;
        }
    }

    pub fn accepts_snapshot(&self, poller_id: u64, revision: u64) -> bool {
        self.geometry_poller.id == poller_id && self.geometry_poller.revision == revision
    }
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

fn snapshot(display_id: u32, bundle_ids: &[String]) -> Option<AppVeilSnapshot> {
    if !geometry_enabled(bundle_ids) {
        return Some(AppVeilSnapshot::default());
    }
    let display = CGDisplay::new(display_id).bounds();
    if display.size.width <= 0.0 || display.size.height <= 0.0 {
        return None;
    }
    let descriptions = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let windows = descriptions
        .get_all_values()
        .into_iter()
        .filter_map(window_from_description)
        .collect();
    Some(snapshot_from_windows(
        windows,
        display_frame(display),
        bundle_ids,
    ))
}

fn window_from_description(description: *const c_void) -> Option<Window> {
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
    Some(Window {
        id,
        frame,
        bundle_id,
    })
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
) -> AppVeilSnapshot {
    let protected: HashSet<&str> = protected_bundle_ids.iter().map(String::as_str).collect();
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
    let protected_window_ids: HashSet<u32> = windows
        .iter()
        .filter(|window| {
            window.bundle_id.as_deref().is_some_and(|bundle_id| {
                !matches!(bundle_id, HOPP_BUNDLE_ID | NOTIFICATION_CENTER_BUNDLE_ID)
                    && protected.contains(bundle_id)
            })
        })
        .map(|window| window.id)
        .collect();
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
    .filter(|(window, _)| protected_window_ids.contains(&window.id))
    .map(|(window, fragments)| AppVeilWindow {
        frame: normalize(window.frame, display),
        visible_fragments: fragments
            .into_iter()
            .map(|fragment| normalize(fragment, display))
            .collect(),
    })
    .collect();
    AppVeilSnapshot {
        windows: veils,
        keyboard_input_blocked: false,
    }
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

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            origin_x: x,
            origin_y: y,
            extent: Extent { width, height },
        }
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

        assert_eq!(GEOMETRY_POLL_INTERVAL, Duration::from_secs(1));
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
            snapshot.windows,
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
                .windows
                .is_empty()
        );
        assert_eq!(
            snapshot_from_windows(
                windows(),
                display,
                &["protected".into(), NOTIFICATION_CENTER_BUNDLE_ID.into()],
            )
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
            .windows
            .is_empty());
        assert_eq!(
            snapshot_at(frame(1340.0, 100.0, 200.0, 400.0)).windows,
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
            snapshot_at(frame(1940.0, 100.0, 200.0, 400.0)).windows,
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
            (current != before).then_some(())
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
