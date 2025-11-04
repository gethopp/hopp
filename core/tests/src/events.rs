use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientPoint {
    pub x: f64,
    pub y: f64,
    // TODO: Make this an enum
    pub pointer: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MouseClickData {
    pub x: f64,
    pub y: f64,
    pub button: u32,
    pub clicks: u32,
    pub down: bool,
    pub shift: bool,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MouseVisibleData {
    pub visible: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct WheelDelta {
    pub deltaX: f64,
    pub deltaY: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeystrokeData {
    pub key: Vec<String>,
    pub meta: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub down: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct TickData {
    pub time: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemoteControlEnabled {
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AddToClipboardData {
    pub is_copy: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardPayload {
    pub packet_id: u64,
    pub total_packets: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PasteFromClipboardData {
    pub data: Option<ClipboardPayload>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ClientEvent {
    MouseMove(ClientPoint),
    MouseClick(MouseClickData),
    MouseVisible(MouseVisibleData),
    Keystroke(KeystrokeData),
    WheelEvent(WheelDelta),
    SharerMove(ClientPoint),
    Tick(TickData),
    TickResponse(TickData),
    RemoteControlEnabled(RemoteControlEnabled),
    AddToClipboard(AddToClipboardData),
    PasteFromClipboard(PasteFromClipboardData),
}
