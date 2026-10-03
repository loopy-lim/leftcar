//! Native input admission and its authenticated Viewer indicator.
use crate::media_sender::MediaSender;
use std::sync::Mutex;

struct GateState {
    enabled: bool,
    retired: bool,
}

pub(crate) struct InputGate {
    state: Mutex<GateState>,
    sender: MediaSender,
}

impl InputGate {
    pub(crate) fn new(sender: MediaSender) -> Self {
        Self {
            state: Mutex::new(GateState {
                enabled: false,
                retired: false,
            }),
            sender,
        }
    }
    pub(crate) fn enabled(&self) -> bool {
        self.state.lock().unwrap().enabled
    }
    pub(crate) fn set_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if state.retired {
            return;
        }
        state.enabled = enabled;
        let _ = self.publish_locked(&state);
    }
    pub(crate) fn retire(&self) {
        // Publish and retirement share admission. On return no delayed old
        // heartbeat/toggle can unlock or publish into a replacement lifetime.
        let mut state = self.state.lock().unwrap();
        state.enabled = false;
        state.retired = true;
    }
    pub(crate) fn refresh_on_control(&self, message: &[u8]) -> bool {
        if message != b"LCK1" && message != b"IDR" {
            return false;
        }
        let _ = self.publish();
        true
    }
    pub(crate) fn publish(&self) -> std::io::Result<()> {
        let state = self.state.lock().unwrap();
        self.publish_locked(&state)
    }
    fn publish_locked(&self, state: &GateState) -> std::io::Result<()> {
        if state.retired {
            return Err(std::io::ErrorKind::NotConnected.into());
        }
        self.sender
            .send(&[b'L', b'C', b'S', b'1', u8::from(state.enabled)])
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_sender::MediaSocket;
    use crate::source_grants::{CaptureAccess, SourceLease};
    use secure_channel::DatagramSealer;
    use std::net::UdpSocket;
    use std::sync::Arc;
    use std::time::Duration;

    fn fixture() -> (InputGate, UdpSocket, DatagramSealer, CaptureAccess) {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(receiver.local_addr().unwrap()).unwrap();
        let key = [95; 32];
        let access = CaptureAccess {
            revision: 1,
            source_id: "fixture".into(),
            owner: "fixture-owner".into(),
            lease: Arc::new(SourceLease::default()),
        };
        let sender =
            MediaSender::new(MediaSocket::Udp(Arc::new(socket)), &key).with_access(&access);
        (
            InputGate::new(sender),
            receiver,
            DatagramSealer::new(secure_channel::media_keys(&key).s2c),
            access,
        )
    }
    fn status(receiver: &UdpSocket, opener: &DatagramSealer) -> Vec<u8> {
        let mut packet = [0; 128];
        let size = receiver.recv(&mut packet).unwrap();
        opener.open(&packet[..size]).unwrap()
    }
    #[test]
    fn host_toggle_publishes_off_then_on_without_a_blocked_key() {
        let (gate, receiver, opener, _) = fixture();
        gate.set_enabled(false);
        assert_eq!(status(&receiver, &opener), b"LCS1\x00");
        gate.set_enabled(true);
        assert_eq!(status(&receiver, &opener), b"LCS1\x01");
        assert!(gate.enabled());
    }
    #[test]
    fn heartbeat_and_idr_repair_a_lost_status_using_current_host_decision() {
        let (gate, receiver, opener, _) = fixture();
        gate.set_enabled(true);
        assert_eq!(status(&receiver, &opener), b"LCS1\x01"); // Drop the toggle update.
        assert!(gate.refresh_on_control(b"LCK1"));
        assert_eq!(status(&receiver, &opener), b"LCS1\x01");
        gate.set_enabled(false);
        assert_eq!(status(&receiver, &opener), b"LCS1\x00");
        assert!(gate.refresh_on_control(b"IDR"));
        assert_eq!(status(&receiver, &opener), b"LCS1\x00");
        assert!(!gate.refresh_on_control(b"unrelated"));
        assert!(receiver.recv(&mut [0; 128]).is_err());
    }
    #[test]
    fn ordinary_status_never_bypasses_a_revoked_source() {
        let (gate, receiver, _, access) = fixture();
        access.lease.invalidate();
        gate.set_enabled(false);
        gate.refresh_on_control(b"LCK1");
        assert_eq!(
            gate.publish().unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(receiver.recv(&mut [0; 128]).is_err());
    }
    #[test]
    fn retiring_an_old_gate_does_not_publish_a_stale_lock_into_its_replacement() {
        let (gate, receiver, opener, _) = fixture();
        gate.set_enabled(true);
        assert_eq!(status(&receiver, &opener), b"LCS1\x01");
        gate.retire();
        gate.refresh_on_control(b"LCK1");
        gate.refresh_on_control(b"IDR");
        gate.set_enabled(true);
        assert!(!gate.enabled());
        assert!(receiver.recv(&mut [0; 128]).is_err());
    }
}
