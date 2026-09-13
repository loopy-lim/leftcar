//! Local TCP bridge for the reliable media paths.
//!
//! The Wi-Fi TCP path binds the device media port on the LAN. The USB path
//! exposes the same loopback listener through `adb forward tcp:N tcp:N`.
//! Host frames are handed directly to the native renderer through a bounded
//! channel, while viewer feedback/control packets use a small UDP side channel
//! and are framed back onto the same TCP connection.

use crate::media_crypto::SharedMediaCrypto;
use crate::net_guard::peer_allowed;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 16 * 1024;
// TCP is reliable, but the renderer still needs a bounded handoff so a
// detached Surface cannot turn a transient lifecycle pause into unbounded
// native memory growth. The renderer drains this queue before decoding.
// The read buffer owns at most one full frame while the renderer queue owns
// at most two more. Even malicious maximum-size frames therefore remain
// bounded to 48 MiB per TCP media session instead of a count-only 4 GiB cap.
const MEDIA_CHANNEL_CAPACITY: usize = 2;

#[cfg(target_os = "android")]
fn bridge_log(message: &str) {
    crate::jni::android_log_info(message.to_owned());
}

#[cfg(not(target_os = "android"))]
fn bridge_log(_message: &str) {}

pub struct PreparedTcpBridge {
    stop: Arc<AtomicBool>,
    control_addr: std::net::SocketAddr,
    media_rx: Receiver<Vec<u8>>,
    crypto: SharedMediaCrypto,
    worker: Option<JoinHandle<()>>,
}

impl PreparedTcpBridge {
    pub fn bind(
        port: u16,
        bind_host: &str,
        allowed_hosts: &str,
        crypto: SharedMediaCrypto,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((bind_host, port))?;
        listener.set_nonblocking(true)?;
        let udp = UdpSocket::bind("127.0.0.1:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(100)))?;
        let control_addr = udp.local_addr()?;
        let (media_tx, media_rx) = mpsc::sync_channel(MEDIA_CHANNEL_CAPACITY);
        let allowed_hosts = allowed_hosts.to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let bridge_crypto = Arc::clone(&crypto);
        let worker = thread::Builder::new()
            .name(format!("leftcar-tcp-{port}"))
            .spawn(move || {
                run_bridge(
                    listener,
                    udp,
                    allowed_hosts,
                    media_tx,
                    bridge_crypto,
                    worker_stop,
                )
            })?;
        Ok(Self {
            stop,
            control_addr,
            media_rx,
            crypto,
            worker: Some(worker),
        })
    }

    /// The session crypto shared with the prepared UDP listener and the
    /// renderer, so the AEAD counters stay continuous across the handoff.
    pub fn shared_crypto(&self) -> SharedMediaCrypto {
        Arc::clone(&self.crypto)
    }

    /// UDP endpoint used by the renderer to send IDR/input/control packets
    /// into the TCP bridge before the first media datagram establishes a
    /// renderer-side peer address.
    pub fn control_addr(&self) -> std::net::SocketAddr {
        self.control_addr
    }

    /// Receive one Host frame without introducing another lossy UDP hop.
    /// TCP framing is already removed by the bridge, so the payload is the
    /// original CFG/control/media datagram consumed by the native renderer.
    pub fn recv_media_timeout(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.media_rx.recv_timeout(timeout) {
            Ok(payload) => Ok(Some(payload)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TCP media bridge disconnected",
            )),
        }
    }

    /// Drain frames while a Surface is detached. They are disposable screen
    /// state, and retaining them would make the first replacement frame stale.
    pub fn drain_media(&self) {
        while self.media_rx.try_recv().is_ok() {}
    }

    fn stop_worker(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PreparedTcpBridge {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

fn run_bridge(
    listener: TcpListener,
    udp: UdpSocket,
    allowed_hosts: String,
    media_tx: SyncSender<Vec<u8>>,
    crypto: SharedMediaCrypto,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer_allowed(Some(peer), &allowed_hosts) {
                    bridge_log(&format!(
                        "TCP media bridge rejected peer {peer}; expected {allowed_hosts}"
                    ));
                    continue;
                }
                bridge_log(&format!(
                    "TCP media bridge accepted Host connection from {peer}"
                ));
                let _ = stream.set_nonblocking(true);
                let connection_stop = Arc::new(AtomicBool::new(false));
                let read_stop = Arc::clone(&connection_stop);
                let write_stop = Arc::clone(&connection_stop);
                let global_read_stop = Arc::clone(&stop);
                let global_write_stop = Arc::clone(&stop);
                let read_media_tx = media_tx.clone();
                let write_udp = match udp.try_clone() {
                    Ok(socket) => socket,
                    Err(_) => continue,
                };
                let write_stream = match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(_) => continue,
                };
                let write_stream = Arc::new(Mutex::new(write_stream));
                let handshake_stream = Arc::clone(&write_stream);
                let read_stream = stream;
                let handshake_crypto = Arc::clone(&crypto);
                let read_thread = thread::spawn(move || {
                    tcp_to_media_channel(
                        read_stream,
                        read_media_tx,
                        &global_read_stop,
                        &read_stop,
                        &handshake_stream,
                        &handshake_crypto,
                    );
                });
                let write_thread = thread::spawn(move || {
                    udp_to_tcp(write_udp, &write_stream, &global_write_stop, &write_stop);
                });
                let _ = read_thread.join();
                connection_stop.store(true, Ordering::SeqCst);
                let _ = write_thread.join();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn tcp_to_media_channel(
    mut stream: TcpStream,
    media_tx: SyncSender<Vec<u8>>,
    global_stop: &AtomicBool,
    connection_stop: &AtomicBool,
    write_stream: &Arc<Mutex<TcpStream>>,
    crypto: &SharedMediaCrypto,
) {
    let mut input = Vec::with_capacity(READ_BUFFER_BYTES);
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    while !global_stop.load(Ordering::SeqCst) && !connection_stop.load(Ordering::SeqCst) {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                input.extend_from_slice(&buffer[..size]);
                loop {
                    if input.len() < 4 {
                        break;
                    }
                    let length = u32::from_be_bytes(input[..4].try_into().unwrap()) as usize;
                    if length == 0 || length > MAX_FRAME_BYTES {
                        connection_stop.store(true, Ordering::SeqCst);
                        return;
                    }
                    let frame_end = 4 + length;
                    if input.len() < frame_end {
                        break;
                    }
                    let payload = input[4..frame_end].to_vec();
                    input.drain(..frame_end);
                    // The framed payload is sealed. Only a frame that opens
                    // under the session key with the LCH1 prefix is echoed;
                    // media frames stay sealed end to end and are opened by
                    // the renderer.
                    if let Some(plaintext) = crypto.open_challenge(&payload) {
                        if let Some(reply) = crypto.seal(&plaintext) {
                            let mut response = Vec::with_capacity(reply.len() + 4);
                            response.extend_from_slice(&(reply.len() as u32).to_be_bytes());
                            response.extend_from_slice(&reply);
                            let write_result = write_stream
                                .lock()
                                .map_err(|_| io::Error::other("TCP writer lock poisoned"))
                                .and_then(|mut socket| socket.write_all(&response));
                            if write_result.is_err() {
                                connection_stop.store(true, Ordering::SeqCst);
                                return;
                            }
                            bridge_log("TCP sealed handshake echoed");
                        }
                    }
                    if payload.starts_with(b"CFG") || payload.starts_with(b"CF2") {
                        bridge_log(&format!(
                            "TCP -> renderer frame={} prefix={:?}",
                            payload.len(),
                            &payload[..payload.len().min(4)]
                        ));
                    }
                    if media_tx.send(payload).is_err() {
                        connection_stop.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
    connection_stop.store(true, Ordering::SeqCst);
}

fn udp_to_tcp(
    udp: UdpSocket,
    stream: &Arc<Mutex<TcpStream>>,
    global_stop: &AtomicBool,
    connection_stop: &AtomicBool,
) {
    let mut payload = vec![0u8; MAX_FRAME_BYTES.min(64 * 1024)];
    while !global_stop.load(Ordering::SeqCst) && !connection_stop.load(Ordering::SeqCst) {
        match udp.recv_from(&mut payload) {
            Ok((size, _peer)) => {
                if size == 0 || size > MAX_FRAME_BYTES {
                    continue;
                }
                let mut frame = Vec::with_capacity(size + 4);
                frame.extend_from_slice(&(size as u32).to_be_bytes());
                frame.extend_from_slice(&payload[..size]);
                if payload.starts_with(b"LCH1")
                    || payload.starts_with(b"LCP1")
                    || payload.starts_with(b"LCF1")
                    || payload.starts_with(b"IDR")
                {
                    bridge_log(&format!(
                        "UDP -> TCP frame={} prefix={:?}",
                        size,
                        &payload[..size.min(4)]
                    ));
                }
                let write_result = stream
                    .lock()
                    .map_err(|_| io::Error::other("TCP writer lock poisoned"))
                    .and_then(|mut socket| socket.write_all(&frame));
                if write_result.is_err() {
                    connection_stop.store(true, Ordering::SeqCst);
                    return;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    connection_stop.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_media_frame_and_session_memory_are_bounded_for_4k() {
        assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
        assert_eq!(MEDIA_CHANNEL_CAPACITY, 2);
        const { assert!(MAX_FRAME_BYTES * (MEDIA_CHANNEL_CAPACITY + 1) <= 48 * 1024 * 1024) };
    }
}
