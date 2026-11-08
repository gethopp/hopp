use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use iced::widget::row;
use iced::{Element, Theme};
use socket_lib::{AudioCaptureMessage, CallId, CameraStartMessage};
use winit::event_loop::EventLoopProxy;

use crate::audio::capturer::list_audio_inputs;
use crate::camera::capturer::CameraCapturer;
use crate::components::split_button::{
    split_button_dropdown_wrap, split_button_sized, SplitButtonItem, SplitButtonSize,
};
use crate::livekit::participant::ParticipantInfo;
use crate::windows::colors::ColorToken;
use crate::UserEvent;

const ICON_MICROPHONE_ON: char = '\u{F105}';
const ICON_MICROPHONE_OFF: char = '\u{F106}';
const ICON_SCREEN_SHARE: char = '\u{F102}';
const ICON_VIDEO: char = '\u{F101}';
const ICON_PHONE_OFF: char = '\u{F103}';

#[derive(Debug, Clone, Copy)]
pub enum CallControlsDensity {
    Regular,
    Compact,
}

impl CallControlsDensity {
    const fn button_size(self) -> SplitButtonSize {
        match self {
            Self::Regular => SplitButtonSize::regular(),
            Self::Compact => SplitButtonSize::compact(),
        }
    }

    const fn spacing(self) -> f32 {
        match self {
            Self::Regular => 8.0,
            Self::Compact => 4.0,
        }
    }

    pub const fn total_width(self) -> f32 {
        match self {
            Self::Regular => 237.0,
            Self::Compact => 189.0,
        }
    }

    const fn camera_dropdown_tail(self) -> f32 {
        match self {
            Self::Regular => 111.0,
            Self::Compact => 87.0,
        }
    }

    const fn mic_dropdown_tail(self) -> f32 {
        match self {
            Self::Regular => 178.0,
            Self::Compact => 140.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CallControlsMessage {
    MicToggle,
    MicDropdownToggle,
    MicDropdownDismiss,
    SelectMic(String),
    VideoToggle(Option<CallId>),
    CameraDropdownToggle,
    CameraDropdownDismiss,
    SelectCamera(Option<CallId>, String),
    ScreenShare,
    OpenScreenSharePicker,
    EndCall(Option<CallId>),
}

#[derive(Default)]
pub struct CallControlsState {
    call_id: Option<CallId>,
    camera_active: bool,
    camera_dropdown_open: bool,
    available_cameras: Vec<socket_lib::CameraDevice>,
    selected_camera_name: Option<String>,
    mic_dropdown_open: bool,
    available_mics: Vec<socket_lib::AudioDevice>,
    selected_mic_name: Option<String>,
}

impl CallControlsState {
    pub fn new(
        call_id: Option<CallId>,
        camera_active: bool,
        selected_camera_name: Option<String>,
        selected_mic_name: Option<String>,
    ) -> Self {
        Self {
            call_id,
            camera_active,
            selected_camera_name,
            selected_mic_name,
            ..Self::default()
        }
    }

    // Preserve the rendered call's identity if delivery happens after the controls change.
    fn end_call_message(&self) -> CallControlsMessage {
        CallControlsMessage::EndCall(self.call_id.clone())
    }

    fn camera_toggle_message(&self) -> CallControlsMessage {
        CallControlsMessage::VideoToggle(self.call_id.clone())
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn test_message(&self, control: socket_lib::TestCallControl) -> CallControlsMessage {
        match control {
            socket_lib::TestCallControl::ToggleMic => CallControlsMessage::MicToggle,
            socket_lib::TestCallControl::ToggleCamera => self.camera_toggle_message(),
            socket_lib::TestCallControl::ToggleScreenShare => CallControlsMessage::ScreenShare,
            socket_lib::TestCallControl::OpenPicker => CallControlsMessage::OpenScreenSharePicker,
            socket_lib::TestCallControl::EndCall => self.end_call_message(),
        }
    }

    pub fn set_call_id(&mut self, call_id: Option<CallId>) {
        self.call_id = call_id;
    }

    pub fn set_camera_active(&mut self, active: bool, device_name: Option<String>) {
        self.camera_active = active;
        if active {
            self.selected_camera_name = device_name;
        }
    }

    pub fn set_selected_mic_name(&mut self, name: Option<String>) {
        self.selected_mic_name = name;
    }

    pub fn dismiss_dropdowns(&mut self) {
        self.camera_dropdown_open = false;
        self.mic_dropdown_open = false;
    }

    pub fn has_open_dropdown(&self) -> bool {
        self.camera_dropdown_open || self.mic_dropdown_open
    }

    pub fn view<'a>(
        &'a self,
        participants: &'a Arc<RwLock<HashMap<String, ParticipantInfo>>>,
        density: CallControlsDensity,
    ) -> Element<'a, CallControlsMessage, Theme, iced::Renderer> {
        let (is_muted, is_screensharing) = participants
            .read()
            .ok()
            .and_then(|participants| {
                participants
                    .get("local")
                    .map(|local| (local.muted(), local.is_screensharing()))
            })
            .unwrap_or((false, false));
        let size = density.button_size();

        let mic = split_button_sized(
            if is_muted {
                ICON_MICROPHONE_OFF
            } else {
                ICON_MICROPHONE_ON
            },
            if is_muted {
                ColorToken::Gray400.to_color()
            } else {
                ColorToken::Orange400.to_color()
            },
            CallControlsMessage::MicToggle,
            Some(CallControlsMessage::MicDropdownToggle),
            self.mic_dropdown_open,
            size,
        );
        let video = split_button_sized(
            ICON_VIDEO,
            if self.camera_active {
                ColorToken::Green400.to_color()
            } else {
                ColorToken::Gray400.to_color()
            },
            self.camera_toggle_message(),
            Some(CallControlsMessage::CameraDropdownToggle),
            self.camera_dropdown_open,
            size,
        );
        let screen = split_button_sized(
            ICON_SCREEN_SHARE,
            if is_screensharing {
                ColorToken::Green400.to_color()
            } else {
                ColorToken::Gray400.to_color()
            },
            CallControlsMessage::ScreenShare,
            Some(CallControlsMessage::OpenScreenSharePicker),
            false,
            size,
        );
        let end_call = split_button_sized(
            ICON_PHONE_OFF,
            ColorToken::Red500.to_color(),
            self.end_call_message(),
            None,
            false,
            size,
        );

        row![mic, video, screen, end_call]
            .spacing(density.spacing())
            .into()
    }

    pub fn wrap_dropdown<'a, Message, Map>(
        &'a self,
        base: Element<'a, Message, Theme, iced::Renderer>,
        map: Map,
        density: CallControlsDensity,
        top_offset: f32,
        trailing_padding: f32,
    ) -> Element<'a, Message, Theme, iced::Renderer>
    where
        Message: Clone + 'a,
        Map: Fn(CallControlsMessage) -> Message + Copy + 'a,
    {
        if self.camera_dropdown_open {
            let items: Vec<SplitButtonItem> = self
                .available_cameras
                .iter()
                .map(|camera| SplitButtonItem {
                    label: camera.name.clone(),
                    selected: self
                        .selected_camera_name
                        .as_ref()
                        .map_or(camera.default, |selected| selected == &camera.name),
                })
                .collect();
            let call_id = self.call_id.clone();
            split_button_dropdown_wrap(
                base,
                &items,
                map(CallControlsMessage::CameraDropdownDismiss),
                move |index| {
                    map(CallControlsMessage::SelectCamera(
                        call_id.clone(),
                        self.available_cameras[index].name.clone(),
                    ))
                },
                top_offset,
                trailing_padding + density.camera_dropdown_tail(),
            )
        } else if self.mic_dropdown_open {
            let items: Vec<SplitButtonItem> = self
                .available_mics
                .iter()
                .map(|mic| SplitButtonItem {
                    label: mic.name.clone(),
                    selected: self
                        .selected_mic_name
                        .as_ref()
                        .map_or(mic.default, |selected| selected == &mic.name),
                })
                .collect();
            split_button_dropdown_wrap(
                base,
                &items,
                map(CallControlsMessage::MicDropdownDismiss),
                move |index| {
                    map(CallControlsMessage::SelectMic(
                        self.available_mics[index].name.clone(),
                    ))
                },
                top_offset,
                trailing_padding + density.mic_dropdown_tail(),
            )
        } else {
            base
        }
    }

    pub fn update(
        &mut self,
        message: CallControlsMessage,
        participants: &Arc<RwLock<HashMap<String, ParticipantInfo>>>,
        event_loop_proxy: &EventLoopProxy<UserEvent>,
    ) {
        let send = |event| {
            if let Err(error) = event_loop_proxy.send_event(event) {
                log::error!("CallControls: failed to send event: {error:?}");
            }
        };

        match message {
            CallControlsMessage::MicToggle => {
                let muted = participants
                    .read()
                    .ok()
                    .and_then(|participants| participants.get("local").map(ParticipantInfo::muted))
                    .unwrap_or(false);
                send(if muted {
                    UserEvent::UnmuteAudio
                } else {
                    UserEvent::MuteAudio
                });
            }
            CallControlsMessage::MicDropdownToggle => {
                self.camera_dropdown_open = false;
                if !self.mic_dropdown_open {
                    self.available_mics = list_audio_inputs();
                }
                self.mic_dropdown_open = !self.mic_dropdown_open;
            }
            CallControlsMessage::MicDropdownDismiss => self.mic_dropdown_open = false,
            CallControlsMessage::SelectMic(name) => {
                self.mic_dropdown_open = false;
                send(UserEvent::StartAudioCapture {
                    msg: AudioCaptureMessage { device_name: name },
                    from_socket: false,
                    request_id: None,
                });
            }
            CallControlsMessage::VideoToggle(Some(call_id)) => send(if self.camera_active {
                UserEvent::StopCamera(call_id)
            } else {
                UserEvent::StartCamera {
                    msg: CameraStartMessage {
                        call_id,
                        device_name: None,
                    },
                    from_socket: false,
                    request_id: None,
                }
            }),
            CallControlsMessage::CameraDropdownToggle => {
                self.mic_dropdown_open = false;
                if !self.camera_dropdown_open {
                    self.available_cameras = CameraCapturer::list_devices();
                }
                self.camera_dropdown_open = !self.camera_dropdown_open;
            }
            CallControlsMessage::CameraDropdownDismiss => self.camera_dropdown_open = false,
            CallControlsMessage::SelectCamera(Some(call_id), name) => {
                self.camera_dropdown_open = false;
                send(UserEvent::StartCamera {
                    msg: CameraStartMessage {
                        call_id,
                        device_name: Some(name),
                    },
                    from_socket: false,
                    request_id: None,
                });
            }
            CallControlsMessage::VideoToggle(None) | CallControlsMessage::SelectCamera(None, _) => {
            }
            CallControlsMessage::ScreenShare => {
                let active = participants
                    .read()
                    .ok()
                    .and_then(|participants| {
                        participants
                            .get("local")
                            .map(ParticipantInfo::is_screensharing)
                    })
                    .unwrap_or(false);
                send(if active {
                    UserEvent::StopScreenShare
                } else {
                    UserEvent::GetAvailableContent
                });
            }
            CallControlsMessage::OpenScreenSharePicker => send(UserEvent::GetAvailableContent),
            CallControlsMessage::EndCall(call_id) => {
                if let Some(call_id) = call_id {
                    send(UserEvent::CallEnd(Some(call_id)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socket_lib::call::{resolve_call_end, CallEndAction};

    #[test]
    fn retained_viewer_rebinds_controls_before_reuse_for_a_new_call() {
        for replace_stream in [false, true] {
            let mut controls = CallControlsState::new(Some("A".into()), false, None, None);
            let queued_hang_up = controls.end_call_message();
            let queued_camera = controls.camera_toggle_message();

            crate::reuse_screensharing_window(
                &mut controls,
                Some("B".into()),
                CallControlsState::set_call_id,
                |controls| {
                    if replace_stream {
                        controls.dismiss_dropdowns();
                    }
                    assert!(matches!(
                        controls.end_call_message(),
                        CallControlsMessage::EndCall(Some(id)) if id == "B"
                    ));
                    assert!(matches!(
                        controls.camera_toggle_message(),
                        CallControlsMessage::VideoToggle(Some(id)) if id == "B"
                    ));
                },
            );
            assert!(matches!(queued_hang_up, CallControlsMessage::EndCall(Some(id)) if id == "A"));
            assert!(
                matches!(queued_camera, CallControlsMessage::VideoToggle(Some(id)) if id == "A")
            );
        }
    }

    #[test]
    fn queued_hang_up_keeps_the_call_shown_by_its_button() {
        let mut controls = CallControlsState::new(Some("A".into()), false, None, None);
        let queued = controls.end_call_message();
        let camera_toggle = controls.camera_toggle_message();
        controls.set_call_id(Some("B".into()));

        let CallControlsMessage::EndCall(Some(origin)) = queued else {
            panic!("hang-up must name its originating call");
        };
        assert_eq!(origin, "A");
        assert!(matches!(camera_toggle, CallControlsMessage::VideoToggle(Some(id)) if id == "A"));
        assert!(
            matches!(controls.camera_toggle_message(), CallControlsMessage::VideoToggle(Some(id)) if id == "B")
        );
        assert!(matches!(
            CallControlsState::default().camera_toggle_message(),
            CallControlsMessage::VideoToggle(None)
        ));
        assert_eq!(
            resolve_call_end(Some(&origin), controls.call_id.as_deref()),
            CallEndAction::Acknowledge("A".into())
        );
        assert!(
            matches!(controls.end_call_message(), CallControlsMessage::EndCall(Some(id)) if id == "B")
        );
        assert!(matches!(
            CallControlsState::default().end_call_message(),
            CallControlsMessage::EndCall(None)
        ));
    }
}
