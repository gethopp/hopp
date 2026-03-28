use std::collections::HashMap;
use std::sync::Arc;

use crate::livekit::participant::ParticipantInfo;

impl std::fmt::Debug for SnapshotSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotSender").finish()
    }
}

pub(crate) struct SnapshotSender {
    socket: socket_lib::SocketSender,
    participants: Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>>,
}

impl SnapshotSender {
    pub fn new(
        socket: socket_lib::SocketSender,
        participants: Arc<std::sync::RwLock<HashMap<String, ParticipantInfo>>>,
    ) -> Self {
        Self {
            socket,
            participants,
        }
    }

    /// Builds a participants snapshot and sends it directly over the socket.
    pub fn send_participants_snapshot(&self) {
        let snapshot = self.build_snapshot();
        if let Err(e) = self
            .socket
            .send(socket_lib::Message::ParticipantsSnapshot(snapshot))
        {
            log::error!("SnapshotSender: Failed to send participants snapshot: {e:?}");
        }
    }

    /// Builds and returns a participants snapshot without sending it.
    pub fn build_snapshot(&self) -> Vec<socket_lib::CoreParticipantState> {
        let guard = self.participants.read().unwrap();
        let mut seen: HashMap<String, socket_lib::CoreParticipantState> = HashMap::new();

        for info in guard.values() {
            let identity = info.identity();
            let parts: Vec<&str> = identity.split(':').collect();
            if parts.len() < 4 {
                continue;
            }
            let user_id = parts[2];
            let track_type = parts[3];

            let entry = seen.entry(user_id.to_string()).or_insert_with(|| {
                socket_lib::CoreParticipantState {
                    identity: identity.to_string(),
                    name: info.name().to_string(),
                    connected: true,
                    muted: false,
                    has_camera: false,
                    is_screensharing: false,
                }
            });

            if track_type == "audio" {
                entry.muted = info.muted();
            }

            entry.has_camera = entry.has_camera || info.camera_active();
            entry.is_screensharing = entry.is_screensharing || info.is_screensharing();
        }

        seen.into_values().collect()
    }
}

impl Clone for SnapshotSender {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            participants: self.participants.clone(),
        }
    }
}
