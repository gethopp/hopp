use socket_lib::{EventSocket, Message, SocketSender};
use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The production wire protocol, with unmatched messages retained for later expectations.
pub struct CoreClient {
    sender: SocketSender,
    socket: EventSocket,
    events: VecDeque<Message>,
    responses: VecDeque<Message>,
    stop: mpsc::Sender<()>,
    keepalive: Option<JoinHandle<()>>,
}

impl CoreClient {
    pub fn connect() -> io::Result<Self> {
        Self::connect_at(crate::SOCKET_PATH.get().expect("socket path initialized"))
    }

    fn connect_at(path: &str) -> io::Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (sender, socket) = loop {
            match socket_lib::connect(path) {
                Ok(connection) => break connection,
                Err(error)
                    if Instant::now() < deadline
                        && matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                        ) =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error),
            }
        };
        let (stop, stopped) = mpsc::channel();
        let ping = sender.clone();
        let keepalive = std::thread::spawn(move || {
            while ping.send(Message::Ping).is_ok() {
                if stopped.recv_timeout(Duration::from_secs(10))
                    != Err(mpsc::RecvTimeoutError::Timeout)
                {
                    break;
                }
            }
        });
        Ok(Self {
            sender,
            socket,
            events: VecDeque::new(),
            responses: VecDeque::new(),
            stop,
            keepalive: Some(keepalive),
        })
    }

    pub fn send(&self, message: Message) -> io::Result<()> {
        self.sender.send(message)
    }

    pub fn request(&mut self, message: Message, timeout: Duration) -> io::Result<Message> {
        let request_id = self.sender.send_request(message)?;
        let response = wait_matching(
            &self.socket.responses,
            &mut self.responses,
            timeout,
            |message| matches!(message, Message::Response { request_id: id, .. } if *id == request_id),
        )
        .map_err(|error| io::Error::new(error.kind(), format!("request {request_id}: {error}")))?;
        match response {
            Message::Response { message, .. } => Ok(*message),
            _ => unreachable!("matched response envelope"),
        }
    }

    pub fn wait_event(
        &mut self,
        timeout: Duration,
        matches: impl FnMut(&Message) -> bool,
    ) -> io::Result<Message> {
        wait_matching(&self.socket.events, &mut self.events, timeout, matches)
    }

    pub fn drain_events(&mut self) {
        self.events.clear();
        while self.socket.events.try_recv().is_ok() {}
    }

    /// Unlike Ping, this must complete a round trip through core's main event loop.
    pub fn probe(&mut self) -> io::Result<Duration> {
        let start = Instant::now();
        match self.request(Message::ListAudioDevices, REQUEST_TIMEOUT)? {
            Message::AudioDeviceList(_) => Ok(start.elapsed()),
            _ => Err(io::Error::other("probe received an unexpected response")),
        }
    }
}

impl Drop for CoreClient {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        self.sender.shutdown();
        if let Some(keepalive) = self.keepalive.take() {
            let _ = keepalive.join();
        }
    }
}

fn wait_matching(
    incoming: &Receiver<Message>,
    pending: &mut VecDeque<Message>,
    timeout: Duration,
    mut matches: impl FnMut(&Message) -> bool,
) -> io::Result<Message> {
    if let Some(index) = pending.iter().position(&mut matches) {
        return Ok(pending.remove(index).expect("existing index"));
    }
    let deadline = Instant::now() + timeout;
    loop {
        let message = incoming
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                let kind = match error {
                    mpsc::RecvTimeoutError::Timeout => io::ErrorKind::TimedOut,
                    mpsc::RecvTimeoutError::Disconnected => io::ErrorKind::BrokenPipe,
                };
                io::Error::new(kind, format!("waiting for core: {error}"))
            })?;
        if matches(&message) {
            return Ok(message);
        }
        pending.push_back(message);
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "waiting for core"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_response_cannot_complete_new_request_and_unmatched_events_survive() {
        let (tx, rx) = mpsc::channel();
        tx.send(Message::Response {
            request_id: 1,
            message: Box::new(Message::AudioDeviceList(Vec::new())),
        })
        .unwrap();
        tx.send(Message::Response {
            request_id: 2,
            message: Box::new(Message::CameraList(Vec::new())),
        })
        .unwrap();
        let mut pending = VecDeque::new();
        let reply = wait_matching(&rx, &mut pending, Duration::from_millis(50), |message| {
            matches!(message, Message::Response { request_id: 2, .. })
        })
        .unwrap();
        assert!(matches!(reply, Message::Response { request_id: 2, .. }));
        assert!(matches!(
            pending.front(),
            Some(Message::Response { request_id: 1, .. })
        ));

        let (tx, rx) = mpsc::channel();
        tx.send(Message::CallEnded("old".into())).unwrap();
        tx.send(Message::DrawingDisabled).unwrap();
        let mut pending = VecDeque::new();
        wait_matching(&rx, &mut pending, Duration::from_millis(50), |message| {
            matches!(message, Message::DrawingDisabled)
        })
        .unwrap();
        assert!(matches!(
            wait_matching(&rx, &mut pending, Duration::ZERO, |message| {
                matches!(message, Message::CallEnded(_))
            })
            .unwrap(),
            Message::CallEnded(id) if id == "old"
        ));
    }
}
