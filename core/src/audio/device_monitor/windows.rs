use log::info;
use std::sync::Arc;
use windows::core::implement;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use winit::event_loop::EventLoopProxy;

use super::DeviceKind;
use crate::UserEvent;

// TODO: revisit this. AI made it.
struct CallbackState {
    kind: DeviceKind,
    proxy: EventLoopProxy<UserEvent>,
}

#[implement(IMMNotificationClient)]
struct DeviceChangeCallback {
    state: Arc<CallbackState>,
}

impl IMMNotificationClient_Impl for DeviceChangeCallback_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        _role: ERole,
        _device_id: &windows::core::PCWSTR,
    ) -> windows::core::Result<()> {
        let expected_flow = match self.state.kind {
            DeviceKind::Output => eRender,
            DeviceKind::Input => eCapture,
        };
        if flow == expected_flow {
            let event = match self.state.kind {
                DeviceKind::Output => UserEvent::DefaultOutputDeviceChanged,
                DeviceKind::Input => UserEvent::DefaultInputDeviceChanged,
            };
            let _ = self.state.proxy.send_event(event);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _device_id: &windows::core::PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _device_id: &windows::core::PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceStateChanged(
        &self,
        _device_id: &windows::core::PCWSTR,
        _new_state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _device_id: &windows::core::PCWSTR,
        _key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

pub struct DeviceMonitor {
    enumerator: IMMDeviceEnumerator,
    callback: IMMNotificationClient,
}

impl DeviceMonitor {
    pub fn new(kind: DeviceKind, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        unsafe {
            // Initialize COM as STA — compatible with winit's OleInitialize.
            // If COM is already initialized (S_FALSE), that's fine.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("CoCreateInstance(MMDeviceEnumerator) failed: {e}"))?;

            let state = Arc::new(CallbackState { kind, proxy });
            let callback_obj: IMMNotificationClient = DeviceChangeCallback { state }.into();

            enumerator
                .RegisterEndpointNotificationCallback(&callback_obj)
                .map_err(|e| format!("RegisterEndpointNotificationCallback failed: {e}"))?;

            let label = match kind {
                DeviceKind::Input => "input",
                DeviceKind::Output => "output",
            };
            info!("Device monitor started ({label})");

            Ok(Self {
                enumerator,
                callback: callback_obj,
            })
        }
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.callback);
        }
    }
}
