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
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_host = expected_host.clone();
        let worker_crypto = Arc::clone(&crypto);
        let worker_port = port;
        let worker = thread::Builder::new()
            .name(format!("leftcar-prepared-udp-{port}"))
            .spawn(move || {
                let mut packet = [0u8; 256];
                prepared_log(&format!(
                    "prepared[{worker_port}]: listener armed, waiting for sealed challenge"
                ));
                while !worker_stop.load(Ordering::SeqCst) {
                    match worker_socket.recv_from(&mut packet) {
                        Ok((size, peer))
                            if peer_allowed(Some(peer), &worker_host) =>
                        {
                            // Only a sealed frame that opens under the
                            // session key — and carries the LCH1 prefix —
                            // is answered. Everything else is dropped
                            // without parsing.
                            if let Some(plaintext) =
                                worker_crypto.open_challenge(&packet[..size])
                            {
                                prepared_log(&format!(
                                    "prepared[{worker_port}]: challenge {size}B opened, echoing"
                                ));
                                *worker_peer.lock().unwrap() = Some(peer);
                                if let Some(reply) = worker_crypto.seal(&plaintext) {
                                    let _ = worker_socket.send_to(&reply, peer);
                                }
                            } else {
                                prepared_log(&format!(
                                    "prepared[{worker_port}]: sealed frame {size}B FAILED to open (key mismatch?)"
                                ));
                            }
                        }
                        Ok(_) => {
                            // Media can arrive immediately after the Host's
                            // proof succeeds. Discard it until the renderer
                            // claims this same socket; bounded startup keeps
                            // the handoff shorter than one GOP.
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

    pub fn into_socket_and_media_crypto(
        mut self,
    ) -> io::Result<(UdpSocket, SharedMediaCrypto, Option<SocketAddr>)> {
        self.stop_worker();
        let _ = self.socket.set_read_timeout(None);
        let peer = *self.peer.lock().unwrap();
        let socket = self.socket.try_clone()?;
        Ok((socket, Arc::clone(&self.crypto), peer))
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

        // Unauthenticated datagrams are never echoed.
        sender
            .send_to(b"LCH1plaintext-forgery", ("127.0.0.1", port))
            .unwrap();
        let mut noise = [0u8; 4];
        sender.send_to(b"keep-alive", ("127.0.0.1", port)).unwrap();
        assert!(sender.recv_from(&mut noise).is_err());

        let (socket, handed_crypto, peer) = prepared.into_socket_and_media_crypto().unwrap();
        assert!(Arc::ptr_eq(&handed_crypto, &crypto));
        assert_eq!(peer, Some(sender.local_addr().unwrap()));
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
