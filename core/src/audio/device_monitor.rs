use log::info;
use objc2_core_audio::{
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
    AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc;

use super::mixer::MixerHandle;

/// Watches for macOS default audio output device changes.
pub struct DeviceMonitor {
    _tx: Box<mpsc::Sender<()>>,
}

unsafe extern "C-unwind" fn listener_proc(
    _id: u32,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let tx = unsafe { &*(client_data as *const mpsc::Sender<()>) };
    let _ = tx.send(());
    0
}

fn property_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

impl DeviceMonitor {
    pub fn new(mixer: MixerHandle) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<()>();
        let tx = Box::new(tx);
        let tx_ptr = &*tx as *const mpsc::Sender<()> as *mut c_void;

        let addr = property_address();

        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject as u32,
                NonNull::from(&addr),
                Some(listener_proc),
                tx_ptr,
            )
        };

        if status != 0 {
            return Err(format!("AudioObjectAddPropertyListener failed: {status}"));
        }

        std::thread::Builder::new()
            .name("audio-device-monitor".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Coalesce rapid-fire notifications
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    while rx.try_recv().is_ok() {}
                    info!("Default audio output device changed, reconnecting...");
                    if let Err(e) = mixer.reconnect() {
                        log::error!("Failed to reconnect audio output: {e}");
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn device monitor thread: {e}"))?;

        Ok(Self { _tx: tx })
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        let addr = property_address();
        unsafe {
            AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject as u32,
                NonNull::from(&addr),
                Some(listener_proc),
                &*self._tx as *const mpsc::Sender<()> as *mut c_void,
            );
        }
    }
}
