use objc2::{rc::Retained, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

struct RunningApplicationsObserverIvars {
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = objc2::MainThreadOnly]
    #[ivars = RunningApplicationsObserverIvars]
    struct RunningApplicationsObserverTarget;

    unsafe impl NSObjectProtocol for RunningApplicationsObserverTarget {}

    impl RunningApplicationsObserverTarget {
        #[unsafe(method(runningApplicationChanged:))]
        fn running_application_changed(&self, _notification: &NSNotification) {
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
    pub fn new(event_loop_proxy: EventLoopProxy<UserEvent>) -> Option<Self> {
        let main_thread_marker = objc2::MainThreadMarker::new()?;
        let target = RunningApplicationsObserverTarget::alloc(main_thread_marker)
            .set_ivars(RunningApplicationsObserverIvars { event_loop_proxy });
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
