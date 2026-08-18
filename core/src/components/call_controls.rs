use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use iced::widget::row;
use iced::{Element, Theme};
use socket_lib::{AudioCaptureMessage, CameraStartMessage};
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
    VideoToggle,
    CameraDropdownToggle,
    CameraDropdownDismiss,
    SelectCamera(String),
    ScreenShare,
    OpenScreenSharePicker,
    EndCall,
}

#[derive(Default)]
pub struct CallControlsState {
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
        camera_active: bool,
        selected_camera_name: Option<String>,
        selected_mic_name: Option<String>,
    ) -> Self {
        Self {
            camera_active,
            selected_camera_name,
            selected_mic_name,
            ..Self::default()
        }
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
            CallControlsMessage::VideoToggle,
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
            CallControlsMessage::EndCall,
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
            split_button_dropdown_wrap(
                base,
                &items,
                map(CallControlsMessage::CameraDropdownDismiss),
                move |index| {
                    map(CallControlsMessage::SelectCamera(
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
                });
            }
            CallControlsMessage::VideoToggle => send(if self.camera_active {
                UserEvent::StopCamera
            } else {
                UserEvent::StartCamera {
                    msg: CameraStartMessage { device_name: None },
                    from_socket: false,
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
            CallControlsMessage::SelectCamera(name) => {
                self.camera_dropdown_open = false;
                send(UserEvent::StartCamera {
                    msg: CameraStartMessage {
                        device_name: Some(name),
                    },
                    from_socket: false,
                });
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
            CallControlsMessage::EndCall => send(UserEvent::CallEnd),
        }
    }
}
