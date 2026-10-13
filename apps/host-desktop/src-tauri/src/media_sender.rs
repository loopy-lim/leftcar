//! The Windows capture sender, shared with platform-independent transport regressions.
use crate::wire;
use secure_channel::DatagramSealer;
use std::io::Write;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const MAX_VIEWER_CONTROL_DATAGRAM: usize = 640;

#[derive(Clone, Copy)]
pub(crate) enum TerminationReason {
    Health = wire::TERMINATION_HEALTH as isize,
    Forced = wire::TERMINATION_FORCED as isize,
    Stopped = wire::TERMINATION_STOPPED as isize,
}

impl TryFrom<u8> for TerminationReason {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            wire::TERMINATION_HEALTH => Ok(Self::Health),
            wire::TERMINATION_FORCED => Ok(Self::Forced),
            wire::TERMINATION_STOPPED => Ok(Self::Stopped),
            _ => Err(format!("invalid termination reason {value}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_grants::{CaptureAccess, SourceLease};
    use std::io::Read;
    use std::net::TcpListener;
    use std::time::Duration;

    fn access() -> CaptureAccess {
        CaptureAccess {
            revision: 1,
            source_id: "fixture".into(),
            owner: "fixture-owner".into(),
            lease: Arc::new(SourceLease::default()),
        }
    }

    #[test]
    fn fenced_udp_source_denies_media_but_delivers_authenticated_terminal() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(receiver.local_addr().unwrap()).unwrap();
        let access = access();
        let key = [91; 32];
        let sender =
            MediaSender::new(MediaSocket::Udp(Arc::new(socket)), &key).with_access(&access);
        assert_eq!(sender.plaintext_limit(), wire::MAX_DATAGRAM);
        access.lease.invalidate();
        assert_eq!(
            sender.send(b"private media").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        sender.send_terminal(TerminationReason::Forced).unwrap();
        let mut frame = [0; 128];
        let size = receiver.recv(&mut frame).unwrap();
        let opener = DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        assert_eq!(opener.open(&frame[..size]).unwrap(), b"LCT1\x02");
        assert!(!access.lease.current());
        assert_eq!(
            sender.send(b"private media").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn fenced_tcp_source_delivers_one_complete_encrypted_terminal_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut receiver, _) = listener.accept().unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let access = access();
        let key = [92; 32];
        let sender = MediaSender::new(MediaSocket::Tcp(Arc::new(Mutex::new(stream))), &key)
            .with_access(&access);
        assert_eq!(sender.plaintext_limit(), wire::MAX_DATAGRAM);
        access.lease.invalidate();
        assert_eq!(
            sender.send(b"private media").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        sender.send_terminal(TerminationReason::Stopped).unwrap();
        let mut length = [0; 4];
        receiver.read_exact(&mut length).unwrap();
        assert_eq!(u32::from_be_bytes(length), 29);
        let mut frame = [0; 29];
        receiver.read_exact(&mut frame).unwrap();
        let opener = DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        assert_eq!(opener.open(&frame).unwrap(), b"LCT1\x03");
        assert!(!access.lease.current());
    }

    #[test]
    fn ordinary_retirement_is_silent_and_explicit_health_is_terminal() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(30)))
            .unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(receiver.local_addr().unwrap()).unwrap();
        let key = [93; 32];
        let sender = MediaSender::new(MediaSocket::Udp(Arc::new(socket)), &key);
        sender.notify_retirement(None).unwrap();
        let mut frame = [0; 128];
        assert!(
            receiver.recv(&mut frame).is_err(),
            "ordinary reconfigure must leave the Viewer alive"
        );
        sender
            .notify_retirement(Some(TerminationReason::Health))
            .unwrap();
        let size = receiver.recv(&mut frame).unwrap();
        let opener = DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        assert_eq!(opener.open(&frame[..size]).unwrap(), b"LCT1\x01");
    }

    #[test]
    fn terminal_cannot_wait_forever_for_a_busy_tcp_writer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_receiver, _) = listener.accept().unwrap();
        let writer = Arc::new(Mutex::new(stream));
        let sender = MediaSender::new(MediaSocket::Tcp(writer.clone()), &[94; 32]);
        let _admitted_media = writer.lock().unwrap();
        let started = Instant::now();
        assert_eq!(
            sender
                .send_terminal(TerminationReason::Forced)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_millis(300));
    }

    #[test]
    fn udp_control_uses_authenticated_ephemeral_port_without_redirecting_media() {
        let media = UdpSocket::bind("127.0.0.1:0").unwrap();
        let control = UdpSocket::bind("127.0.0.1:0").unwrap();
        let host = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        for socket in [&media, &control, &*host] {
            socket
                .set_read_timeout(Some(Duration::from_millis(30)))
                .unwrap();
        }
        assert_ne!(media.local_addr().unwrap(), control.local_addr().unwrap());
        let key = [97; 32];
        let keys = secure_channel::media_keys(&key);
        let opener = DatagramSealer::new(keys.c2s);
        let viewer_tx = DatagramSealer::new(keys.c2s);
        let viewer_rx = DatagramSealer::new(keys.s2c);
        let access = access();
        let sender = MediaSender::new(
            MediaSocket::UdpTo(host.clone(), media.local_addr().unwrap()),
            &key,
        )
        .with_access(&access);
        assert_eq!(sender.plaintext_limit(), wire::MAX_DATAGRAM);
        assert_eq!(sender.udp_peer(), Some(media.local_addr().unwrap()));

        // The host socket receives the viewer's distinct control port. A
        // connected media socket would filter this datagram at the OS layer.
        let mut reliable = b"LCI1".to_vec();
        reliable.extend_from_slice(&1u32.to_be_bytes());
        reliable.extend_from_slice(&[5, 1]); // Reliable release-all.
        let invalid = DatagramSealer::new([98; 32]).seal(&reliable).unwrap();
        control
            .send_to(&invalid, host.local_addr().unwrap())
            .unwrap();
        let mut bytes = [0; 640];
        let (size, source) = host.recv_from(&mut bytes).unwrap();
        assert!(sender
            .authenticate_udp_control(&opener, &bytes[..size], source)
            .is_none());
        assert!(
            control.recv(&mut bytes).is_err(),
            "unauthenticated reflection"
        );

        let oversized = viewer_tx.seal(&[0; MAX_VIEWER_CONTROL_DATAGRAM]).unwrap();
        control
            .send_to(&oversized, host.local_addr().unwrap())
            .unwrap();
        let mut full_datagram = [0; 65_535];
        let (size, source) = host.recv_from(&mut full_datagram).unwrap();
        assert!(sender
            .authenticate_udp_control(&opener, &full_datagram[..size], source)
            .is_none());
        assert_eq!(
            opener.open(&full_datagram[..size]).unwrap().len(),
            MAX_VIEWER_CONTROL_DATAGRAM
        );

        let envelope = viewer_tx.seal(&reliable).unwrap();
        control
            .send_to(&envelope, host.local_addr().unwrap())
            .unwrap();
        let (size, source) = host.recv_from(&mut bytes).unwrap();
        let (message, reply) = sender
            .authenticate_udp_control(&opener, &bytes[..size], source)
            .unwrap();
        assert_eq!(reply.udp_peer(), Some(control.local_addr().unwrap()));
        assert_eq!(sender.udp_peer(), Some(media.local_addr().unwrap()));
        let mut sequencer = wire::InputSequencer::default();
        assert!(matches!(
            sequencer.accept(&message),
            wire::InputDecision::ApplyAndAck { sequence: 1, .. }
        ));
        reply.send(&wire::input_ack(1, true)).unwrap();
        let size = control.recv(&mut bytes).unwrap();
        assert_eq!(
            viewer_rx.open(&bytes[..size]).unwrap(),
            wire::input_ack(1, true)
        );
        assert!(
            media.recv(&mut bytes).is_err(),
            "control ACK went to media port"
        );

        sender.send(b"media stays here").unwrap();
        let size = media.recv(&mut bytes).unwrap();
        // Same opener proves reply and media share one nonce sequence.
        assert_eq!(viewer_rx.open(&bytes[..size]).unwrap(), b"media stays here");
        assert!(
            control.recv(&mut bytes).is_err(),
            "media followed the control port"
        );
        assert!(
            sender
                .authenticate_udp_control(&opener, &envelope, source)
                .is_none(),
            "replayed control was accepted"
        );

        access.lease.invalidate();
        assert_eq!(
            reply.send(b"ACK").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        sender.send_terminal(TerminationReason::Forced).unwrap();
        let size = media.recv(&mut bytes).unwrap();
        assert_eq!(viewer_rx.open(&bytes[..size]).unwrap(), b"LCT1\x02");
    }

    #[test]
    fn udp_control_rejects_a_different_ip_before_consuming_a_valid_nonce() {
        let control = UdpSocket::bind("127.0.0.1:0").unwrap();
        let host = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        host.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let key = [99; 32];
        let keys = secure_channel::media_keys(&key);
        let viewer = DatagramSealer::new(keys.c2s);
        let opener = DatagramSealer::new(keys.c2s);
        let sender = MediaSender::new(
            MediaSocket::UdpTo(host.clone(), "192.0.2.1:5000".parse().unwrap()),
            &key,
        );
        assert_eq!(sender.plaintext_limit(), wire::REMOTE_DATAGRAM);
        let envelope = viewer.seal(b"IDR").unwrap();
        control
            .send_to(&envelope, host.local_addr().unwrap())
            .unwrap();
        let mut packet = [0; 128];
        let (size, source) = host.recv_from(&mut packet).unwrap();
        assert!(sender
            .authenticate_udp_control(&opener, &packet[..size], source)
            .is_none());
        assert_eq!(opener.open(&packet[..size]).unwrap(), b"IDR");
    }

    #[cfg(unix)]
    #[test]
    fn partial_tcp_submission_fences_every_clone_before_a_later_frame() {
        use std::os::fd::AsRawFd;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut receiver, _) = listener.accept().unwrap();
        let small: libc::c_int = 1024;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &small as *const _ as *const _,
                    std::mem::size_of_val(&small) as _,
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    receiver.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &small as *const _ as *const _,
                    std::mem::size_of_val(&small) as _,
                )
            },
            0
        );
        stream
            .set_write_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let sender = MediaSender::new(MediaSocket::Tcp(Arc::new(Mutex::new(stream))), &[96; 32]);
        let payload = vec![7; secure_channel::MAX_DATAGRAM]; // Valid sealer input, then actual socket backpressure.
        assert!(
            sender.send(&payload).is_err(),
            "fixture did not backpressure the real writer"
        );
        let mut prefix = [0; 4];
        receiver.read_exact(&mut prefix).unwrap();
        let expected = u32::from_be_bytes(prefix) as usize;
        let mut remainder = Vec::new();
        let mut packet = [0; 8192];
        while let Ok(size) = receiver.read(&mut packet) {
            if size == 0 {
                break;
            }
            remainder.extend_from_slice(&packet[..size]);
        }
        assert!(
            remainder.len() < expected,
            "the first write was not partial"
        );
        assert_eq!(
            sender.clone().send(b"next frame").unwrap_err().kind(),
            std::io::ErrorKind::NotConnected,
            "a clone appended a fresh prefix inside the incomplete frame"
        );
        assert_eq!(
            sender
                .send_terminal(TerminationReason::Forced)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotConnected,
            "even fixed terminal output cannot repair corrupted TCP framing"
        );
    }
}

#[derive(Clone)]
pub(crate) enum MediaSocket {
    Udp(Arc<UdpSocket>),
    /// Unconnected receive socket with an immutable media destination. The
    /// viewer sends authenticated control from a different ephemeral port.
    UdpTo(Arc<UdpSocket>, SocketAddr),
    Tcp(Arc<Mutex<TcpStream>>),
}

/// Every host → viewer media send is sealed here — the lowest-level sender —
/// so capture output, input acks, and termination notices share one AEAD
/// boundary. TCP keeps its plaintext 4-byte length prefix; only the payload
/// is sealed.
#[derive(Clone)]
pub(crate) struct MediaSender {
    socket: MediaSocket,
    /// s2c sealer under the HKDF-derived directional key — the raw session
    /// key is never used directly, so the two directions can never collide
    /// on a (key, nonce) pair. All media/control/terminal clones of this session
    /// share this exact sealer, counter sequence and replay-window lifetime.
    tx: Arc<DatagramSealer>,
    access: Option<Arc<crate::source_grants::SourceLease>>,
    failed_tcp: Arc<AtomicBool>,
}

impl MediaSender {
    pub(crate) fn new(socket: MediaSocket, media_key: &[u8; 32]) -> Self {
        let keys = secure_channel::media_keys(media_key);
        Self {
            socket,
            tx: Arc::new(DatagramSealer::new(keys.s2c)),
            access: None,
            failed_tcp: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn with_access(mut self, access: &crate::source_grants::CaptureAccess) -> Self {
        self.access = Some(access.lease.clone());
        self
    }

    pub(crate) fn is_tcp(&self) -> bool {
        matches!(self.socket, MediaSocket::Tcp(_))
    }

    pub(crate) fn plaintext_limit(&self) -> usize {
        match &self.socket {
            MediaSocket::Tcp(_) => wire::MAX_DATAGRAM,
            MediaSocket::Udp(socket) => {
                wire::media_plaintext_limit(socket.peer_addr().ok().map(|peer| peer.ip()))
            }
            MediaSocket::UdpTo(_, peer) => wire::media_plaintext_limit(Some(peer.ip())),
        }
    }

    pub(crate) fn udp_peer(&self) -> Option<SocketAddr> {
        match &self.socket {
            MediaSocket::Udp(socket) => socket.peer_addr().ok(),
            MediaSocket::UdpTo(_, peer) => Some(*peer),
            MediaSocket::Tcp(_) => None,
        }
    }

    /// A reply endpoint exists only after the source IP and session AEAD both
    /// pass. It shares the original sender's nonce sequence and source lease,
    /// while the original media destination stays unchanged.
    pub(crate) fn authenticate_udp_control(
        &self,
        opener: &DatagramSealer,
        envelope: &[u8],
        source: SocketAddr,
    ) -> Option<(Vec<u8>, Self)> {
        let MediaSocket::UdpTo(socket, peer) = &self.socket else {
            return None;
        };
        if source.ip() != peer.ip() || envelope.len() > MAX_VIEWER_CONTROL_DATAGRAM {
            return None;
        }
        let message = opener.open(envelope).ok()?;
        let mut reply = self.clone();
        reply.socket = MediaSocket::UdpTo(socket.clone(), source);
        Some((message, reply))
    }

    pub(crate) fn prepare(&self, packet: &[u8]) -> std::io::Result<Vec<u8>> {
        wire::seal_media_packet(&self.tx, packet, self.is_tcp())
    }

    /// One ordinary datagram, or one existing length-prefixed TCP frame.
    /// No GSO, scatter/gather-as-batch or uncertain automatic resend.
    pub(crate) fn submit_sealed(&self, envelope: &[u8]) -> std::io::Result<usize> {
        let _permission = self
            .access
            .as_ref()
            .map(|lease| {
                lease.enter().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "source authorization revoked",
                    )
                })
            })
            .transpose()?;
        match &self.socket {
            MediaSocket::Udp(socket) => socket.send(envelope),
            MediaSocket::UdpTo(socket, peer) => socket.send_to(envelope, peer),
            MediaSocket::Tcp(_) => self
                .write_tcp_envelope(envelope, Instant::now() + Duration::from_millis(200))
                .map(|_| envelope.len()),
        }
    }

    pub(crate) fn send_terminal(&self, reason: TerminationReason) -> std::io::Result<()> {
        // This is the sole lifecycle-only authority: fixed LCT1 bytes, existing
        // session sealer and established endpoint. No arbitrary payload or new
        // source operation can pass through it after the source is fenced.
        let notice = wire::termination(reason as u8);
        let deadline = Instant::now() + Duration::from_millis(100);
        match &self.socket {
            MediaSocket::Udp(socket) | MediaSocket::UdpTo(socket, _) => {
                let old_timeout = socket.write_timeout()?;
                socket.set_write_timeout(Some(Duration::from_millis(20)))?;
                let result = (|| {
                    for attempt in 0..2 {
                        if attempt > 0 {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        if Instant::now() >= deadline {
                            return Err(std::io::ErrorKind::TimedOut.into());
                        }
                        let envelope = self.prepare(&notice)?;
                        let submitted = match &self.socket {
                            MediaSocket::UdpTo(_, peer) => socket.send_to(&envelope, peer)?,
                            _ => socket.send(&envelope)?,
                        };
                        wire::complete_plaintext_send(notice.len(), envelope.len(), submitted)?;
                    }
                    Ok(())
                })();
                let restored = socket.set_write_timeout(old_timeout);
                result.and(restored)
            }
            MediaSocket::Tcp(_) => {
                let envelope = self.prepare(&notice)?;
                self.write_tcp_envelope(&envelope, deadline)
            }
        }
    }

    // Private transport machinery shared by ordinary authorized output and
    // fixed terminal output. Authority is enforced by their separate public
    // entry points; no caller can submit a general payload without its lease.
    fn write_tcp_envelope(&self, envelope: &[u8], deadline: Instant) -> std::io::Result<()> {
        let MediaSocket::Tcp(writer) = &self.socket else {
            return Err(std::io::ErrorKind::InvalidInput.into());
        };
        let mut stream = loop {
            if self.failed_tcp.load(Ordering::Acquire) {
                return Err(std::io::ErrorKind::NotConnected.into());
            }
            match writer.try_lock() {
                Ok(stream) => break stream,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(std::io::Error::other("TCP media writer lock poisoned"))
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        if self.failed_tcp.load(Ordering::Acquire) {
            return Err(std::io::ErrorKind::NotConnected.into());
        }
        let old_timeout = stream.write_timeout()?;
        let result = (|| {
            let mut offset = 0;
            while offset < envelope.len() {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(std::io::ErrorKind::TimedOut)?;
                stream.set_write_timeout(Some(
                    old_timeout.map_or(remaining, |old| old.min(remaining)),
                ))?;
                match stream.write(&envelope[offset..]) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(sent) => offset += sent,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })();
        if result.is_err() {
            // Commit failure before releasing the writer lock. A partial
            // prefix/payload can never be followed by another framed packet;
            // shutdown also wakes this exact session's input receiver.
            self.failed_tcp.store(true, Ordering::Release);
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        let restored = stream.set_write_timeout(old_timeout);
        result.and(restored)
    }

    pub(crate) fn notify_retirement(
        &self,
        reason: Option<TerminationReason>,
    ) -> std::io::Result<()> {
        reason.map_or(Ok(()), |reason| self.send_terminal(reason))
    }

    /// Input acknowledgements/notices retain the plaintext return contract.
    pub(crate) fn send(&self, packet: &[u8]) -> std::io::Result<usize> {
        let envelope = self.prepare(packet)?;
        let submitted = self.submit_sealed(&envelope)?;
        wire::complete_plaintext_send(packet.len(), envelope.len(), submitted)
    }
}
