use livekit::webrtc::prelude::RtcVideoTrack;
use std::sync::mpsc::channel::{Receiver, Sender};

enum SendFramesMessage {
    Stop,
}

#[derive(Debug)]
pub struct VideoClient {
    sender: Sender<SendFramesMessage>,
}

impl VideoClient {
    pub fn new(track: RtcVideoTrack) -> Self {
        Self {}
    }
}

async fn send_frames(receiver: Receiver<SendFramesMessage>, track: RtcVideoTrack) {}
