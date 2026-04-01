use log::info;
use objc2_core_audio::{
    kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;
use super::DeviceKind;

type ListenerFn =
    unsafe extern "C-unwind" fn(u32, u32, NonNull<AudioObjectPropertyAddress>, *mut c_void) -> i32;

/// Watches for a single macOS default audio device change (input OR output).
pub struct DeviceMonitor {
    kind: DeviceKind,
    listener_fn: ListenerFn,
    _proxy: Box<EventLoopProxy<UserEvent>>,
}

unsafe extern "C-unwind" fn output_listener_proc(
    _id: u32,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let proxy = unsafe { &*(client_data as *const EventLoopProxy<UserEvent>) };
    let _ = proxy.send_event(UserEvent::DefaultOutputDeviceChanged);
    0
}

unsafe extern "C-unwind" fn input_listener_proc(
    _id: u32,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let proxy = unsafe { &*(client_data as *const EventLoopProxy<UserEvent>) };
    let _ = proxy.send_event(UserEvent::DefaultInputDeviceChanged);
    0
}

fn property_address(kind: DeviceKind) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: match kind {
            DeviceKind::Input => kAudioHardwarePropertyDefaultInputDevice,
            DeviceKind::Output => kAudioHardwarePropertyDefaultOutputDevice,
        },
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

impl DeviceMonitor {
    pub fn new(kind: DeviceKind, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let proxy = Box::new(proxy);
        let proxy_ptr = &*proxy as *const EventLoopProxy<UserEvent> as *mut c_void;
        let addr = property_address(kind);

        let listener_fn: ListenerFn = match kind {
            DeviceKind::Input => input_listener_proc,
            DeviceKind::Output => output_listener_proc,
        };

        let label = match kind {
            DeviceKind::Input => "input",
            DeviceKind::Output => "output",
        };

        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject as u32,
                NonNull::from(&addr),
                Some(listener_fn),
                proxy_ptr,
            )
        };
        if status != 0 {
            return Err(format!(
                "AudioObjectAddPropertyListener ({label}) failed: {status}"
            ));
        }

        info!("Device monitor started ({label})");
        Ok(Self {
            kind,
            listener_fn,
            _proxy: proxy,
        })
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        let addr = property_address(self.kind);
        unsafe {
            AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject as u32,
                NonNull::from(&addr),
                Some(self.listener_fn),
                &*self._proxy as *const EventLoopProxy<UserEvent> as *mut c_void,
            );
        }
    }
}
