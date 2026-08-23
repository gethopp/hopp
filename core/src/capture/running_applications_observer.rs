use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use objc2::{rc::Retained, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSRunningApplication, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidLaunchApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

struct RunningApplicationsObserverIvars {
    event_loop_proxy: EventLoopProxy<UserEvent>,
    bundle_ids: Arc<Mutex<HashSet<String>>>,
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = objc2::MainThreadOnly]
    #[ivars = RunningApplicationsObserverIvars]
    struct RunningApplicationsObserverTarget;

    unsafe impl NSObjectProtocol for RunningApplicationsObserverTarget {}

    impl RunningApplicationsObserverTarget {
        #[unsafe(method(runningApplicationChanged:))]
        fn running_application_changed(&self, notification: &NSNotification) {
            let bundle_id = notification
                .userInfo()
                .and_then(|user_info| unsafe { user_info.objectForKey(NSWorkspaceApplicationKey) })
                .and_then(|application| {
                    application
                        .downcast_ref::<NSRunningApplication>()
                        .and_then(|application| application.bundleIdentifier())
                });
            let Some(bundle_id) = bundle_id else {
                return;
            };
            let bundle_id = bundle_id.to_string();
            if !self.ivars().bundle_ids.lock().unwrap().contains(&bundle_id) {
                log::info!("RunningApplicationsObserver: ignoring unprotected app: {bundle_id}");
                return;
            }
            log::info!("RunningApplicationsObserver: protected app launched/terminated: {bundle_id}");
            let _ = self
                .ivars()
                .event_loop_proxy
                .send_event(UserEvent::RefreshAppVeilFilter);
        }
    }
);

pub struct RunningApplicationsObserver {
    workspace: Retained<NSWorkspace>,
    target: Retained<RunningApplicationsObserverTarget>,
}

impl RunningApplicationsObserver {
    pub fn new(
        event_loop_proxy: EventLoopProxy<UserEvent>,
        bundle_ids: Arc<Mutex<HashSet<String>>>,
    ) -> Option<Self> {
        let Some(main_thread_marker) = objc2::MainThreadMarker::new() else {
            log::error!(
                "RunningApplicationsObserver: not created on the main thread; application-launch filtering will be degraded"
            );
            return None;
        };
        let target = RunningApplicationsObserverTarget::alloc(main_thread_marker).set_ivars(
            RunningApplicationsObserverIvars {
                event_loop_proxy,
                bundle_ids,
            },
        );
        let target: Retained<RunningApplicationsObserverTarget> =
            unsafe { objc2::msg_send![super(target), init] };
        let workspace = NSWorkspace::sharedWorkspace();
        unsafe {
            let notification_center = workspace.notificationCenter();
            for notification in [
                NSWorkspaceDidLaunchApplicationNotification,
                NSWorkspaceDidTerminateApplicationNotification,
            ] {
                notification_center.addObserver_selector_name_object(
                    target.as_super(),
                    objc2::sel!(runningApplicationChanged:),
                    Some(notification),
                    None,
                );
            }
        }
        Some(Self { workspace, target })
    }
}

impl Drop for RunningApplicationsObserver {
    fn drop(&mut self) {
        unsafe {
            self.workspace
                .notificationCenter()
                .removeObserver(self.target.as_super());
        }
    }
}
