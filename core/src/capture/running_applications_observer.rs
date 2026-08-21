use objc2::{rc::Retained, runtime::AnyObject, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSObject,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSString,
};
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
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_running_applications(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut std::ffi::c_void,
        ) {
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
    key_path: Retained<NSString>,
}

impl RunningApplicationsObserver {
    pub fn new(event_loop_proxy: EventLoopProxy<UserEvent>) -> Option<Self> {
        let main_thread_marker = objc2::MainThreadMarker::new()?;
        let target = RunningApplicationsObserverTarget::alloc(main_thread_marker)
            .set_ivars(RunningApplicationsObserverIvars { event_loop_proxy });
        let target: Retained<RunningApplicationsObserverTarget> =
            unsafe { objc2::msg_send![super(target), init] };
        let workspace = NSWorkspace::sharedWorkspace();
        let key_path = NSString::from_str("runningApplications");
        unsafe {
            workspace.as_super().addObserver_forKeyPath_options_context(
                target.as_super(),
                &key_path,
                NSKeyValueObservingOptions::New,
                std::ptr::null_mut(),
            );
        }
        Some(Self {
            workspace,
            target,
            key_path,
        })
    }
}

impl Drop for RunningApplicationsObserver {
    fn drop(&mut self) {
        unsafe {
            self.workspace
                .as_super()
                .removeObserver_forKeyPath(self.target.as_super(), &self.key_path);
        }
    }
}
