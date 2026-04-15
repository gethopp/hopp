use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};

/// Wraps an NSProcessInfo activity token so it can be stored in `AppData` behind a `Mutex`.
struct ActivityToken(Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: The token is only stored and dropped behind Mutex<AppData>,
// ensuring exclusive access. The underlying ObjC object is not mutated across threads.
unsafe impl Send for ActivityToken {}
unsafe impl Sync for ActivityToken {}

/// Manages macOS sleep prevention during active calls.
///
/// When enabled, prevents both display and system idle sleep by holding an
/// `NSProcessInfo` activity with the appropriate options. Dropping or disabling
/// releases the assertion and restores normal sleep behavior.
#[derive(Default)]
pub struct SleepPrevention {
    activity: Option<ActivityToken>,
}

impl SleepPrevention {
    pub fn new() -> Self {
        Self::default()
    }

    /// Prevents display and system idle sleep. Idempotent: repeated calls are no-ops.
    pub fn enable(&mut self) {
        if self.activity.is_some() {
            log::info!("sleep_prevention: already enabled, skipping");
            return;
        }

        let process_info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("Hopp pairing call in progress");

        let activity = process_info.beginActivityWithOptions_reason(
            NSActivityOptions::IdleDisplaySleepDisabled
                | NSActivityOptions::IdleSystemSleepDisabled,
            &reason,
        );

        self.activity = Some(ActivityToken(activity));
        log::info!("sleep_prevention: enabled — display and system sleep prevented");
    }

    /// Releases the sleep prevention assertion. Idempotent: safe to call when already disabled.
    pub fn disable(&mut self) {
        if let Some(token) = self.activity.take() {
            // SAFETY: The token is the exact object returned by beginActivityWithOptions_reason.
            unsafe { NSProcessInfo::processInfo().endActivity(&token.0) };
            log::info!("sleep_prevention: disabled — normal sleep behavior restored");
        }
    }
}

impl Drop for SleepPrevention {
    fn drop(&mut self) {
        self.disable();
    }
}
