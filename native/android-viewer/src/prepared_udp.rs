//! UDP listener preflight for race-free stream startup.
//!
//! The Host proves that a viewer owns its media port before capture starts.
//! Android used to open the stream Activity only after writing `startStream`,
//! so a slow Activity/Surface creation could miss the bounded challenge and
//! leave a black window. This listener binds first, echoes only authenticated
//! Host candidates, then hands the same socket and challenge token to the
//! renderer once its Surface exists.

use crate::net_guard::{hosts_are_valid, peer_allowed};
use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const CHALLENGE_PREFIX: &[u8] = b"LCH1";
const MAX_CHALLENGE_BYTES: usize = 128;

pub struct PreparedUdpReceiver {
    socket: UdpSocket,
    expected_host: String,
    token: Arc<Mutex<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PreparedUdpReceiver {
    pub fn bind(port: u16, expected_host: String) -> io::Result<Self> {
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
        let token = Arc::new(Mutex::new(Vec::new()));
        let worker_token = Arc::clone(&token);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_host = expected_host.clone();
        let worker = thread::Builder::new()
            .name(format!("leftcar-prepared-udp-{port}"))
            .spawn(move || {
                let mut packet = [0u8; 256];
                while !worker_stop.load(Ordering::SeqCst) {
                    match worker_socket.recv_from(&mut packet) {
                        Ok((size, peer))
                            if peer_allowed(Some(peer), &worker_host)
                                && size > CHALLENGE_PREFIX.len()
                                && size <= MAX_CHALLENGE_BYTES
                                && packet[..size].starts_with(CHALLENGE_PREFIX) =>
                        {
                            let challenge = &packet[..size];
                            *worker_token.lock().unwrap() =
                                challenge[CHALLENGE_PREFIX.len()..].to_vec();
                            let _ = worker_socket.send_to(challenge, peer);
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
            token,
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

    pub fn into_socket_and_token(mut self) -> io::Result<(UdpSocket, Vec<u8>)> {
        self.stop_worker();
        let _ = self.socket.set_read_timeout(None);
        let token = self.token.lock().unwrap().clone();
        let socket = self.socket.try_clone()?;
        Ok((socket, token))
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

    #[test]
    fn echoes_host_challenge_then_hands_socket_to_renderer() {
        let prepared = PreparedUdpReceiver::bind(0, "127.0.0.1".into()).unwrap();
        let port = prepared.port().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let challenge = b"LCH1race-free-token";
        sender.send_to(challenge, ("127.0.0.1", port)).unwrap();
        let mut response = [0u8; 128];
        let (size, _) = sender.recv_from(&mut response).unwrap();
        assert_eq!(&response[..size], challenge);

        let (socket, token) = prepared.into_socket_and_token().unwrap();
        assert_eq!(token, b"race-free-token");
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sender.send_to(b"media", ("127.0.0.1", port)).unwrap();
        let mut media = [0u8; 16];
        let (size, _) = socket.recv_from(&mut media).unwrap();
        assert_eq!(&media[..size], b"media");
    }

    #[test]
    fn rejects_non_ip_host_before_binding() {
        let error = match PreparedUdpReceiver::bind(0, "leftcar.local".into()) {
            Ok(_) => panic!("hostname must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
