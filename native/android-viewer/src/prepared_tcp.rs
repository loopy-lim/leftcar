//! Local TCP bridge for the reliable media paths.
//!
//! The Wi-Fi TCP path binds the device media port on the LAN. The USB path
//! exposes the same loopback listener through `adb forward tcp:N tcp:N`.
//! Host frames are handed directly to the native renderer through a bounded
//! channel, while viewer feedback/control packets use a small UDP side channel
//! and are framed back onto the same TCP connection.

use crate::net_guard::peer_allowed;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 16 * 1024;
// TCP is reliable, but the renderer still needs a bounded handoff so a
// detached Surface cannot turn a transient lifecycle pause into unbounded
// native memory growth. The renderer drains this queue before decoding.
const MEDIA_CHANNEL_CAPACITY: usize = 256;

#[cfg(target_os = "android")]
fn bridge_log(message: &str) {
    let Ok(message) = std::ffi::CString::new(message) else {
        return;
    };
    unsafe {
        extern "C" {
            fn __android_log_print(
                priority: i32,
                tag: *const std::ffi::c_char,
                format: *const std::ffi::c_char,
                ...
            ) -> i32;
        }
        let tag = b"LeftcarNative\0";
        let format = b"%s\0";
        __android_log_print(
            4,
            tag.as_ptr().cast(),
            format.as_ptr().cast(),
            message.as_ptr(),
        );
    }
}

#[cfg(not(target_os = "android"))]
fn bridge_log(_message: &str) {}

pub struct PreparedTcpBridge {
    stop: Arc<AtomicBool>,
    control_addr: std::net::SocketAddr,
    media_rx: Receiver<Vec<u8>>,
    worker: Option<JoinHandle<()>>,
}

impl PreparedTcpBridge {
    pub fn bind(port: u16, bind_host: &str, allowed_hosts: &str) -> io::Result<Self> {
        let listener = TcpListener::bind((bind_host, port))?;
        listener.set_nonblocking(true)?;
        let udp = UdpSocket::bind("127.0.0.1:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(100)))?;
        let control_addr = udp.local_addr()?;
        let (media_tx, media_rx) = mpsc::sync_channel(MEDIA_CHANNEL_CAPACITY);
        let allowed_hosts = allowed_hosts.to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name(format!("leftcar-tcp-{port}"))
            .spawn(move || run_bridge(listener, udp, allowed_hosts, media_tx, worker_stop))?;
        Ok(Self {
            stop,
            control_addr,
            media_rx,
            worker: Some(worker),
        })
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
                let read_thread = thread::spawn(move || {
                    tcp_to_media_channel(
                        read_stream,
                        read_media_tx,
                        &global_read_stop,
                        &read_stop,
                        &handshake_stream,
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
                    if payload.starts_with(b"LCH1") {
                        // Host media setup uses a framed challenge before it
                        // starts capture. Echo it on the same TCP connection;
                        // the renderer also receives it so it can authenticate
                        // reverse IDR/input/feedback traffic.
                        let mut response = Vec::with_capacity(payload.len() + 4);
                        response.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                        response.extend_from_slice(&payload);
                        let write_result = write_stream
                            .lock()
                            .map_err(|_| io::Error::other("TCP writer lock poisoned"))
                            .and_then(|mut socket| socket.write_all(&response));
                        if write_result.is_err() {
                            connection_stop.store(true, Ordering::SeqCst);
                            return;
                        }
                        bridge_log(&format!(
                            "TCP handshake echoed frame={} prefix={:?}",
                            payload.len(),
                            &payload[..payload.len().min(4)]
                        ));
                    }
                    if payload.starts_with(b"LCH1") || payload.starts_with(b"CFG") {
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
