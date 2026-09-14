//! UDP listener preflight for race-free stream startup.
//!
//! The Host proves that a viewer owns its media port before capture starts.
//! Android used to open the stream Activity only after writing `startStream`,
//! so a slow Activity/Surface creation could miss the bounded challenge and
//! leave a black window. This listener binds first, answers only datagrams
//! that open under the session media key (the sealed `LCH1` challenge), then
//! hands the same socket and the shared session crypto to the renderer once
//! its Surface exists. Sharing one `MediaSessionCrypto` instance keeps the
//! AEAD counters continuous across the handoff.

use crate::media_crypto::{SharedMediaCrypto, CHALLENGE_PREFIX};
use crate::net_guard::{hosts_are_valid, peer_allowed};
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// 증명 경로 진단 로그. jni::log_info! 매크로는 jni 모듈이
/// `cfg(any(android, test))`로 게이트되어 있어 이 상시 컴파일 모듈에서는
/// 못 쓴다 — prepared_tcp::bridge_log와 같은 지역 분할을 쓴다.
#[cfg(target_os = "android")]
fn prepared_log(message: &str) {
    crate::jni::android_log_info(message.to_owned());
}

#[cfg(not(target_os = "android"))]
fn prepared_log(_message: &str) {}

/// Sealed challenge + AEAD overhead; genuine host challenges fit easily.
const MAX_CHALLENGE_BYTES: usize = 192;

/// Sealed media datagrams run counter(8) + tag(16) + payload, so a full
/// host fragment fits well under 2 KiB. The old 256 B worker buffer
/// truncated every early media frame into an unopenable datagram.
const WORKER_READ_BYTES: usize = 2_048;

/// Early-media backlog bound. The Host's startup IDR burst (~90 datagrams)
/// rides the prepared→renderer handoff; 512 datagrams cover it plus jitter
/// many times over while bounding memory under 1 MiB. Overflow drops the
/// oldest, matching the live-edge policy the renderer applies anyway.
const PREPARED_BACKLOG_CAP: usize = 512;

/// Recognize a Host `LCH1` reachability challenge from an already-opened
/// plaintext. Shared by the preflight worker and both renderers so the
/// acceptance rule cannot drift.
pub fn is_challenge(plaintext: &[u8]) -> bool {
    plaintext.starts_with(CHALLENGE_PREFIX) && plaintext.len() <= MAX_CHALLENGE_BYTES
}

/// `Some(plaintext)` when the opened frame is a reachability challenge, so
/// callers can echo the identical plaintext sealed in their own direction.
pub fn is_challenge_packet(plaintext: &[u8]) -> Option<&[u8]> {
    is_challenge(plaintext).then_some(plaintext)
}

pub fn split_ports(base_port: u16) -> Result<(u16, u16), &'static str> {
    let right_port = base_port
        .checked_add(1)
        .ok_or("split stream requires two consecutive ports")?;
    Ok((base_port, right_port))
}

pub struct PreparedUdpReceiver {
    socket: UdpSocket,
    expected_host: String,
    crypto: SharedMediaCrypto,
    peer: Arc<Mutex<Option<SocketAddr>>>,
    pending: Arc<Mutex<VecDeque<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PreparedUdpReceiver {
    pub fn bind(port: u16, expected_host: String, crypto: SharedMediaCrypto) -> io::Result<Self> {
        if !hosts_are_valid(&expected_host) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected host must be a bare IP address",
            ));
        }

        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        // Keep cancellation/handoff quantization below one 60 Hz frame.
        socket.set_read_timeout(Some(Duration::from_millis(10)))?;
        let worker_socket = socket.try_clone()?;
        let peer = Arc::new(Mutex::new(None));
        let worker_peer = Arc::clone(&peer);
        let pending: Arc<Mutex<VecDeque<Vec<u8>>>> = Arc::new(Mutex::new(VecDeque::new()));
        let worker_pending = Arc::clone(&pending);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_host = expected_host.clone();
        let worker_crypto = Arc::clone(&crypto);
        let worker_port = port;
        let worker = thread::Builder::new()
            .name(format!("leftcar-prepared-udp-{port}"))
            .spawn(move || {
                let mut packet = [0u8; WORKER_READ_BYTES];
                prepared_log(&format!(
                    "prepared[{worker_port}]: listener armed, waiting for sealed challenge"
                ));
                let mut buffering_logged = false;
                while !worker_stop.load(Ordering::SeqCst) {
                    match worker_socket.recv_from(&mut packet) {
                        Ok((size, peer))
                            if peer_allowed(Some(peer), &worker_host) =>
                        {
                            // Only a sealed frame that opens under the
                            // session key — and carries the LCH1 prefix —
                            // is answered. Everything else is early media
                            // racing the renderer handoff.
                            // Classify without consuming the receive window:
                            // open_challenge advances it, and a frame the
                            // worker opens is a replay the renderer can
                            // never accept from the backlog.
                            match worker_crypto.authenticate_packet(&packet[..size]) {
                                Some(plaintext)
                                    if plaintext.starts_with(CHALLENGE_PREFIX) =>
                                {
                                    // The echo proves the key handshake to the
                                    // rest of the session: without this flag the
                                    // renderer never seeds host_peer, so IDR
                                    // requests and 1Hz feedback never leave the
                                    // device and the Host kills the stream as
                                    // "feedback timeout".
                                    if let Some(opened) =
                                        worker_crypto.open_challenge(&packet[..size])
                                    {
                                        worker_crypto.establish();
                                        prepared_log(&format!(
                                            "prepared[{worker_port}]: challenge {size}B opened, echoing"
                                        ));
                                        *worker_peer.lock().unwrap() = Some(peer);
                                        if let Some(reply) = worker_crypto.seal(&opened) {
                                            let _ = worker_socket.send_to(&reply, peer);
                                        }
                                    }
                                }
                                Some(_) => {
                                    // Real media racing the renderer handoff:
                                    // buffer the still-sealed datagram so the
                                    // claim replays it instead of the Host's
                                    // startup IDR being lost.
                                    let mut queue = worker_pending.lock().unwrap();
                                    if !buffering_logged {
                                        buffering_logged = true;
                                        prepared_log(&format!(
                                            "prepared[{worker_port}]: buffering early media before renderer claim"
                                        ));
                                    }
                                    if queue.len() >= PREPARED_BACKLOG_CAP {
                                        queue.pop_front();
                                    }
                                    queue.push_back(packet[..size].to_vec());
                                }
                                // Anything else is forged or truncated: drop.
                                None => {}
                            }
                        }
                        Ok(_) => {
                            // Datagrams from unexpected peers stay dropped.
                        }
                        Err(error)
                            if error.kind() == io::ErrorKind::WouldBlock
                                || error.kind() == io::ErrorKind::TimedOut => {}
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self {
            socket,
            expected_host,
            crypto,
            peer,
            pending,
            stop,
            worker: Some(worker),
        })
    }

    pub fn port(&self) -> io::Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    pub fn expected_host(&self) -> &str {
        &self.expected_host
    }

    pub fn into_socket_media_crypto_and_backlog(
        mut self,
    ) -> io::Result<(
        UdpSocket,
        SharedMediaCrypto,
        Option<SocketAddr>,
        VecDeque<Vec<u8>>,
    )> {
        self.stop_worker();
        let _ = self.socket.set_read_timeout(None);
        let peer = *self.peer.lock().unwrap();
        let backlog = std::mem::take(&mut *self.pending.lock().unwrap());
        let socket = self.socket.try_clone()?;
        Ok((socket, Arc::clone(&self.crypto), peer, backlog))
    }

    fn stop_worker(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PreparedUdpReceiver {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_crypto::MediaSessionCrypto;

    fn test_key(bytes: u8) -> [u8; 32] {
        (bytes..bytes + 32).collect::<Vec<u8>>().try_into().unwrap()
    }

    #[test]
    fn split_ports_are_consecutive_and_bounded() {
        assert_eq!(split_ports(5002), Ok((5002, 5003)));
        assert!(split_ports(u16::MAX).is_err());
    }

    #[test]
    fn echoes_sealed_challenge_then_hands_socket_and_crypto_to_renderer() {
        let crypto: SharedMediaCrypto = Arc::new(MediaSessionCrypto::new(test_key(1)));
        let prepared =
            PreparedUdpReceiver::bind(0, "127.0.0.1".into(), Arc::clone(&crypto)).unwrap();
        let port = prepared.port().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        // Host side: seal the LCH1 challenge with the derived s2c key.
        let host_keys = secure_channel::media_keys(&test_key(1));
        let host_tx = secure_channel::DatagramSealer::new(host_keys.s2c);
        let challenge = [CHALLENGE_PREFIX, b"race-free-nonce".as_slice()].concat();
        sender
            .send_to(&host_tx.seal(&challenge).unwrap(), ("127.0.0.1", port))
            .unwrap();
        let mut response = [0u8; 256];
        let (size, _) = sender.recv_from(&mut response).unwrap();
        // The echo is sealed under the viewer's own direction; the host
        // opens it with its own receiving window.
        let host_rx = secure_channel::DatagramSealer::new(host_keys.c2s);
        assert_eq!(host_rx.open(&response[..size]).unwrap(), challenge);

        // Unauthenticated datagrams are neither echoed nor buffered.
        sender
            .send_to(b"LCH1plaintext-forgery", ("127.0.0.1", port))
            .unwrap();
        let mut noise = [0u8; 4];
        sender.send_to(b"keep-alive", ("127.0.0.1", port)).unwrap();
        assert!(sender.recv_from(&mut noise).is_err());

        // Pre-claim media is buffered for the claim instead of dropped — the
        // Host's startup IDR rides exactly this window. The buffering check
        // must not consume the receive window: the claim re-opens the frame.
        sender
            .send_to(&host_tx.seal(b"early media").unwrap(), ("127.0.0.1", port))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));

        let (socket, handed_crypto, peer, mut backlog) = prepared
            .into_socket_media_crypto_and_backlog()
            .unwrap();
        assert!(Arc::ptr_eq(&handed_crypto, &crypto));
        assert_eq!(peer, Some(sender.local_addr().unwrap()));
        // The echoed challenge marks the handshake complete for the session.
        assert!(crypto.is_established());
        assert_eq!(backlog.len(), 1);
        let early = backlog.pop_front().unwrap();
        assert_eq!(crypto.open(&early).unwrap(), b"early media");
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sender
            .send_to(&host_tx.seal(b"media").unwrap(), ("127.0.0.1", port))
            .unwrap();
        let mut media = [0u8; 64];
        let (size, _) = socket.recv_from(&mut media).unwrap();
        assert_eq!(crypto.open(&media[..size]).unwrap(), b"media");
    }

    #[test]
    fn rejects_non_ip_host_before_binding() {
        let crypto: SharedMediaCrypto = Arc::new(MediaSessionCrypto::new(test_key(2)));
        let error = match PreparedUdpReceiver::bind(0, "leftcar.local".into(), crypto) {
            Ok(_) => panic!("hostname must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn split_media_receive_buffer_holds_a_large_recovery_pair() {
        assert_eq!(
            crate::socket_tuning::split_media_receive_buffer_bytes(),
            4 * 1024 * 1024
        );
    }
}
