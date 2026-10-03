//! Android UsbAccessory fd bridge.
//!
//! The renderer continues to use its existing UDP-side control socket and
//! media queue. This bridge replaces only the physical transport: channel 1
//! carries media/control datagrams and channel 0 carries the local JSON
//! control connection.

use crate::media_crypto::SharedMediaCrypto;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_FRAME_BYTES: usize = usb_mux::MAX_FRAME_BYTES;
const READ_BUFFER_BYTES: usize = 64 * 1024;
// One frame may be assembling in MuxDecoder while two complete frames wait
// for the renderer: at most 48 MiB of AOAP media payload per session.
const MEDIA_CHANNEL_CAPACITY: usize = 2;
// Matches the capture shim's `udpMediaFragmentPayloadBytes`: the reliable
// TCP-style media frames arriving over AOAP carry one whole access unit,
// while the native renderer only consumes shim-shaped UDP datagrams. We
// re-segment into the same 1,400-byte datagrams (33-byte fragment header).
const FRAGMENT_DATAGRAM_BYTES: usize = 1_400;
const FRAGMENT_HEADER_BYTES: usize = 33;
const FRAGMENT_PAYLOAD_BYTES: usize = FRAGMENT_DATAGRAM_BYTES - FRAGMENT_HEADER_BYTES;

/// Split one whole access-unit frame (L2 logical layout: marker, AU id,
/// "L2", capture/encode wall clocks, H.264 payload) into the shim's
/// fragmented UDP datagram shape so the renderer's existing fragment
/// pipeline can consume it. Mirrors the shim's `writePacket(isFrame: true)`
/// fragmentation exactly, minus parity (reliable transports send none).
pub(crate) fn fragment_au_frame(payload: &[u8]) -> Option<Vec<Vec<u8>>> {
    if payload.len() <= FRAGMENT_HEADER_BYTES
        || payload[0] != b'G'
        || payload[3] != b'L'
        || payload[4] != b'2'
    {
        return None;
    }
    let body = &payload[21..];
    if body.is_empty() {
        return None;
    }
    let count = body.len().div_ceil(FRAGMENT_PAYLOAD_BYTES);
    let header = &payload[1..21];
    // The renderer only echoes this field back; it never interprets it
    // locally, so zero keeps the shape valid without a host clock source.
    let send_wall_ms_be = [0u8; 8];
    let mut datagrams = Vec::with_capacity(count);
    for (index, chunk) in body.chunks(FRAGMENT_PAYLOAD_BYTES).enumerate() {
        let mut datagram = Vec::with_capacity(FRAGMENT_HEADER_BYTES + chunk.len());
        datagram.push(b'G');
        datagram.extend_from_slice(&(index as u16).to_be_bytes());
        datagram.extend_from_slice(&(count as u16).to_be_bytes());
        datagram.extend_from_slice(header);
        datagram.extend_from_slice(&send_wall_ms_be);
        datagram.extend_from_slice(chunk);
        datagrams.push(datagram);
    }
    Some(datagrams)
}

#[derive(Clone)]
struct MediaRoute {
    generation: u64,
    crypto: SharedMediaCrypto,
    media_tx: SyncSender<Vec<u8>>,
    udp: Arc<UdpSocket>,
}

/// One physical accessory owner. Logical renderer cancellation must not
/// duplicate or detach its blocking Android driver reader.
pub struct UsbBridge {
    stop: Arc<AtomicBool>,
    control_port: u16,
    route: Arc<Mutex<Option<MediaRoute>>>,
    generation: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

/// Only this incarnation can consume media or send renderer feedback. The
/// physical service outlives the lease and can be reused by the next session.
pub struct UsbMediaLease {
    bridge: Arc<UsbBridge>,
    generation: u64,
    control_addr: std::net::SocketAddr,
    media_rx: Receiver<Vec<u8>>,
    media_port: Option<u16>,
}

impl UsbBridge {
    pub fn start(fd: i32) -> io::Result<Self> {
        Self::start_with_control_limit(fd, MAX_FRAME_BYTES)
    }

    fn start_with_control_limit(fd: i32, max_control_line_bytes: usize) -> io::Result<Self> {
        if fd < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid USB fd",
            ));
        }
        let duplicate = unsafe { libc::dup(fd) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(duplicate) };
        let writer = Arc::new(Mutex::new(file.try_clone()?));
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let control_port = listener.local_addr()?.port();
        let (control_tx, control_rx) = mpsc::sync_channel(64);
        let route = Arc::new(Mutex::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_route = Arc::clone(&route);
        let worker_generation = Arc::clone(&generation);
        let worker = thread::Builder::new()
            .name(format!("leftcar-usb-bridge-{control_port}"))
            .spawn(move || {
                let reader_stop = Arc::clone(&worker_stop);
                let reader_writer = Arc::clone(&writer);
                let reader_route = Arc::clone(&worker_route);
                let reader_generation = Arc::clone(&worker_generation);
                let reader = thread::spawn(move || {
                    read_accessory(
                        file,
                        reader_writer,
                        control_tx,
                        reader_stop,
                        &reader_route,
                        &reader_generation,
                    );
                });
                let writer_stop = Arc::clone(&worker_stop);
                let udp_writer = Arc::clone(&writer);
                let writer_route = Arc::clone(&worker_route);
                let writer_thread = thread::spawn(move || {
                    write_udp_media(writer_route, udp_writer, writer_stop);
                });
                run_control_proxy(
                    listener,
                    writer,
                    control_rx,
                    &worker_stop,
                    &worker_generation,
                    max_control_line_bytes,
                );
                worker_stop.store(true, Ordering::SeqCst);
                let _ = reader.join();
                let _ = writer_thread.join();
            })?;
        Ok(Self {
            stop,
            control_port,
            route,
            generation,
            worker: Some(worker),
        })
    }

    pub fn prepare_media(self: &Arc<Self>, media_key: [u8; 32]) -> io::Result<UsbMediaLease> {
        if !self.is_running() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "USB accessory disconnected",
            ));
        }
        let udp = UdpSocket::bind("127.0.0.1:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(50)))?;
        let control_addr = udp.local_addr()?;
        let (media_tx, media_rx) = mpsc::sync_channel(MEDIA_CHANNEL_CAPACITY);
        let mut route = self.route.lock().unwrap();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *route = Some(MediaRoute {
            generation,
            crypto: Arc::new(crate::media_crypto::MediaSessionCrypto::new(media_key)),
            media_tx,
            udp: Arc::new(udp),
        });
        drop(route);
        Ok(UsbMediaLease {
            bridge: Arc::clone(self),
            generation,
            control_addr,
            media_rx,
            media_port: None,
        })
    }

    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }

    pub fn is_retired(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }

    pub fn stop_after_detach(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.route.lock().unwrap().take();
    }

    /// The prepared UDP listener and this physical reader use one counter
    /// sequence for the current media incarnation.
    pub fn set_media_crypto(&self, crypto: SharedMediaCrypto) {
        if let Some(route) = self.route.lock().unwrap().as_mut() {
            route.crypto = crypto;
        }
    }

    pub fn shared_crypto(&self) -> Option<SharedMediaCrypto> {
        self.route
            .lock()
            .unwrap()
            .as_ref()
            .map(|route| Arc::clone(&route.crypto))
    }

    pub fn control_port(&self) -> u16 {
        self.control_port
    }
}

impl UsbMediaLease {
    pub fn bind_media_port(&mut self, port: u16) {
        self.media_port = Some(port);
    }

    pub fn matches_media_port(&self, port: u16) -> bool {
        self.media_port.unwrap_or(0) == port
    }

    pub fn is_unbound(&self) -> bool {
        self.media_port.is_none()
    }

    pub fn is_current(&self) -> bool {
        self.bridge.is_running() && route_is_current(&self.bridge.route, self.generation)
    }

    pub fn control_addr(&self) -> std::net::SocketAddr {
        self.control_addr
    }

    pub fn recv_media_timeout(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        if !self.is_current() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "USB media lease superseded",
            ));
        }
        match self.media_rx.recv_timeout(timeout) {
            Ok(payload) if self.is_current() => Ok(Some(payload)),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "USB media lease superseded",
            )),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "USB media bridge disconnected",
            )),
        }
    }

    pub fn drain_media(&self) {
        while self.media_rx.try_recv().is_ok() {}
    }
}

impl Drop for UsbMediaLease {
    fn drop(&mut self) {
        let mut route = self.bridge.route.lock().unwrap();
        if route
            .as_ref()
            .is_some_and(|route| route.generation == self.generation)
        {
            route.take();
        }
    }
}

impl Drop for UsbBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Android accessory drivers can ignore O_NONBLOCK and have no poll
        // callback. Detach wakes their interruptible driver read; joining a
        // still-attached fd here would block the UI. Keep one process owner
        // across logical sessions and retire this worker on physical detach.
        let _ = self.worker.take();
    }
}

fn route_is_current(route: &Mutex<Option<MediaRoute>>, generation: u64) -> bool {
    route
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|route| route.generation == generation)
}

fn forward_media(
    payload: Vec<u8>,
    generation: u64,
    route: &Mutex<Option<MediaRoute>>,
    stop: &AtomicBool,
) {
    let Some(current) = route.lock().unwrap().clone() else {
        return;
    };
    if current.generation != generation {
        return;
    }
    let mut payload = payload;
    while !stop.load(Ordering::SeqCst) && route_is_current(route, current.generation) {
        match current.media_tx.try_send(payload) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(returned)) => {
                payload = returned;
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn send_until_stopped<T>(sender: &SyncSender<T>, mut payload: T, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::SeqCst) {
        match sender.try_send(payload) {
            Ok(()) => return true,
            Err(TrySendError::Full(returned)) => {
                payload = returned;
                thread::sleep(Duration::from_millis(2));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    false
}

fn read_accessory(
    mut file: File,
    writer: Arc<Mutex<File>>,
    control_tx: SyncSender<(u64, Vec<u8>)>,
    stop: Arc<AtomicBool>,
    route: &Mutex<Option<MediaRoute>>,
    control_generation: &AtomicU64,
) {
    let mut decoder = usb_mux::MuxDecoder::new();
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    'accessory: while !stop.load(Ordering::SeqCst) {
        let size = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => size,
            Err(_) => break,
        };
        let frames = match decoder.feed(&buffer[..size]) {
            Ok(frames) => frames,
            Err(_) => break,
        };
        for frame in frames {
            if frame.channel == usb_mux::CHANNEL_CONTROL {
                if !send_until_stopped(
                    &control_tx,
                    (control_generation.load(Ordering::SeqCst), frame.payload),
                    &stop,
                ) {
                    break 'accessory;
                }
                continue;
            }
            let Some(current) = route.lock().unwrap().clone() else {
                continue;
            };
            if frame.channel == usb_mux::CHANNEL_MEDIA {
                // Only a sealed frame that opens under the session key with
                // the LCH1 prefix is echoed to the Host; media stays sealed
                // end to end and is opened by the renderer.
                //
                // The reachability challenge arrives once per session, before
                // any media. Probe-opening later frames would consume their
                // AEAD receive counters here while the renderer opens the
                // same sealed frame again — every media datagram would then
                // die as a replay. After establishment, forward every
                // channel-1 frame sealed.
                let crypto = Arc::clone(&current.crypto);
                if !crypto.is_established() {
                    if let Some(plaintext) = crypto.open_challenge(&frame.payload) {
                        // The UDP preflight receiver marks the shared
                        // instance established when it verifies the
                        // sealed challenge (prepared_udp). The USB bridge
                        // answers the same handshake, so it must set the
                        // same gate here — without it every renderer
                        // control send (initial IDR request, feedback,
                        // probes) stays gated off and the host health
                        // check kills the session at 6s.
                        crypto.establish();
                        if let Some(reply) = crypto.seal(&plaintext) {
                            let Ok(bytes) = usb_mux::encode(usb_mux::CHANNEL_MEDIA, &reply) else {
                                break 'accessory;
                            };
                            let Ok(mut output) = writer.lock() else {
                                break 'accessory;
                            };
                            if !route_is_current(route, current.generation) {
                                continue;
                            }
                            if output.write_all(&bytes).is_err() {
                                break 'accessory;
                            }
                        }
                        // The echoed challenge is a control exchange, not
                        // renderer media.
                        continue;
                    }
                }
            }
            // Whole access units exceed the renderer's datagram buffers, so
            // they must re-enter it as shim-shaped fragments.
            if frame.channel == usb_mux::CHANNEL_MEDIA {
                if let Some(datagrams) = fragment_au_frame(&frame.payload) {
                    for datagram in datagrams {
                        forward_media(datagram, current.generation, route, &stop);
                    }
                    continue;
                }
            }
            forward_media(frame.payload, current.generation, route, &stop);
        }
    }
    stop.store(true, Ordering::SeqCst);
}

fn write_udp_media(
    route: Arc<Mutex<Option<MediaRoute>>>,
    writer: Arc<Mutex<File>>,
    stop: Arc<AtomicBool>,
) {
    let mut packet = vec![0u8; MAX_FRAME_BYTES.min(64 * 1024)];
    while !stop.load(Ordering::SeqCst) {
        let Some(current) = route.lock().unwrap().clone() else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        let size = match current.udp.recv(&mut packet) {
            Ok(size) if size > 0 && size <= MAX_FRAME_BYTES => size,
            Ok(_) => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => break,
        };
        if !route_is_current(&route, current.generation) {
            continue;
        }
        let Ok(bytes) = usb_mux::encode(usb_mux::CHANNEL_MEDIA, &packet[..size]) else {
            continue;
        };
        let Ok(mut output) = writer.lock() else { break };
        if !route_is_current(&route, current.generation) {
            continue;
        }
        if output.write_all(&bytes).is_err() {
            break;
        }
    }
    stop.store(true, Ordering::SeqCst);
}

fn run_control_proxy(
    listener: TcpListener,
    writer: Arc<Mutex<File>>,
    control_rx: Receiver<(u64, Vec<u8>)>,
    stop: &AtomicBool,
    control_generation: &AtomicU64,
    max_control_line_bytes: usize,
) {
    let mut stream: Option<TcpStream> = None;
    let mut input = Vec::new();
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut scanned = 0;
    let mut generation = control_generation.load(Ordering::SeqCst);
    while !stop.load(Ordering::SeqCst) {
        let current_generation = control_generation.load(Ordering::SeqCst);
        if generation != current_generation {
            stream = None;
            input.clear();
            scanned = 0;
            generation = current_generation;
        }
        if stream.is_none() {
            if let Ok((candidate, _)) = listener.accept() {
                let _ = candidate.set_nonblocking(true);
                stream = Some(candidate);
            }
        }
        if let Some(current) = stream.as_mut() {
            match current.read(&mut buffer) {
                Ok(0) => {
                    stream = None;
                    input.clear();
                    scanned = 0;
                }
                Ok(size) => {
                    input.extend_from_slice(&buffer[..size]);
                    while let Some(newline) = input[scanned..]
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map(|offset| scanned + offset)
                    {
                        if newline > max_control_line_bytes {
                            stream = None;
                            input.clear();
                            break;
                        }
                        let line: Vec<u8> = input.drain(..=newline).collect();
                        scanned = 0;
                        let payload = line.strip_suffix(b"\n").unwrap_or(&line).to_vec();
                        if let Ok(frame) = usb_mux::encode(usb_mux::CHANNEL_CONTROL, &payload) {
                            let Ok(mut output) = writer.lock() else {
                                return;
                            };
                            if output.write_all(&frame).is_err() {
                                return;
                            }
                        }
                    }
                    if input.len() > max_control_line_bytes {
                        stream = None;
                        input.clear();
                    }
                    scanned = input.len();
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => {
                    stream = None;
                    input.clear();
                    scanned = 0;
                }
            }
        }
        loop {
            match control_rx.try_recv() {
                Ok((response_generation, payload)) => {
                    if response_generation != generation {
                        continue;
                    }
                    if let Some(current) = stream.as_mut() {
                        if current.write_all(&payload).is_err() {
                            stream = None;
                            input.clear();
                            scanned = 0;
                            break;
                        }
                        let _ = current.flush();
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_media_lease_cannot_cancel_or_consume_the_next_session() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;

        let (viewer, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let key = crate::media_crypto::test_media_key(9);
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let old = bridge.prepare_media(key).unwrap();
        let current = bridge.prepare_media(key).unwrap();
        assert_ne!(old.control_addr(), current.control_addr());
        assert!(old.recv_media_timeout(Duration::ZERO).is_err());
        drop(old);
        assert!(current.is_current());
        let host_tx = secure_channel::DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        let challenge = host_tx.seal(b"LCH1quiet-accessory").unwrap();
        host.write_all(&usb_mux::encode(usb_mux::CHANNEL_MEDIA, &challenge).unwrap())
            .unwrap();
        let mut header = [0u8; 5];
        host.read_exact(&mut header).unwrap();
        let mut reply = vec![0u8; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize];
        host.read_exact(&mut reply).unwrap();
        assert!(bridge.is_running());
        bridge.stop_after_detach();
        drop(host); // Physical disconnect releases the driver's blocking read.
    }

    #[test]
    fn a_new_session_releases_old_queue_backpressure_and_rejects_old_udp_feedback() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let old_key = crate::media_crypto::test_media_key(4);
        let old = bridge.prepare_media(old_key).unwrap();
        let old_tx = secure_channel::DatagramSealer::new(secure_channel::media_keys(&old_key).s2c);
        let send = |host: &mut UnixStream, tx: &secure_channel::DatagramSealer, data: &[u8]| {
            host.write_all(
                &usb_mux::encode(usb_mux::CHANNEL_MEDIA, &tx.seal(data).unwrap()).unwrap(),
            )
            .unwrap();
        };
        let receive = |host: &mut UnixStream| {
            let mut header = [0u8; 5];
            host.read_exact(&mut header).unwrap();
            let mut payload =
                vec![0u8; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize];
            host.read_exact(&mut payload).unwrap();
            payload
        };
        send(&mut host, &old_tx, b"LCH1old");
        receive(&mut host);
        for _ in 0..3 {
            send(&mut host, &old_tx, b"old media");
        }
        thread::sleep(Duration::from_millis(50));

        let new_key = crate::media_crypto::test_media_key(5);
        let current = bridge.prepare_media(new_key).unwrap();
        let new_tx = secure_channel::DatagramSealer::new(secure_channel::media_keys(&new_key).s2c);
        send(&mut host, &new_tx, b"LCH1new");
        let new_rx = secure_channel::DatagramSealer::new(secure_channel::media_keys(&new_key).c2s);
        assert_eq!(new_rx.open(&receive(&mut host)).unwrap(), b"LCH1new");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"old feedback", old.control_addr()).unwrap();
        sender
            .send_to(b"current feedback", current.control_addr())
            .unwrap();
        assert_eq!(receive(&mut host), b"current feedback");
        assert!(old.recv_media_timeout(Duration::ZERO).is_err());
        bridge.stop_after_detach();
        drop(host);
    }

    #[test]
    fn a_new_control_connection_never_inherits_an_unfinished_line() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let _lease = bridge
            .prepare_media(crate::media_crypto::test_media_key(7))
            .unwrap();
        let mut old = TcpStream::connect(("127.0.0.1", bridge.control_port())).unwrap();
        old.write_all(b"unfinished-old-request").unwrap();
        old.shutdown(std::net::Shutdown::Both).unwrap();
        let mut current = TcpStream::connect(("127.0.0.1", bridge.control_port())).unwrap();
        current.write_all(b"current-request\n").unwrap();
        let mut header = [0u8; 5];
        host.read_exact(&mut header).unwrap();
        let mut request = vec![0u8; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize];
        host.read_exact(&mut request).unwrap();
        assert_eq!(request, b"current-request");
        bridge.stop_after_detach();
        drop(host);
    }

    #[test]
    fn an_oversized_unterminated_control_line_is_rejected() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, host) = UnixStream::pair().unwrap();
        let bridge = Arc::new(UsbBridge::start_with_control_limit(viewer.as_raw_fd(), 32).unwrap());
        let _lease = bridge
            .prepare_media(crate::media_crypto::test_media_key(7))
            .unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", bridge.control_port())).unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        // A peer may never delimit its first command. The native loopback
        // proxy still needs a byte bound before the Host sees that command.
        let _ = client.write_all(&[b'x'; 33]);
        let result = client.read(&mut [0u8; 1]);
        assert!(
            matches!(result, Ok(0))
                || result.is_err_and(|error| error.kind() == io::ErrorKind::ConnectionReset),
            "oversized partial commands must close the connection"
        );
        bridge.stop_after_detach();
        drop(host);
    }

    #[test]
    fn accessory_write_failure_marks_the_physical_owner_unavailable() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, host) = UnixStream::pair().unwrap();
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let lease = bridge
            .prepare_media(crate::media_crypto::test_media_key(3))
            .unwrap();
        viewer.shutdown(std::net::Shutdown::Write).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"feedback", lease.control_addr()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while bridge.is_running() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !bridge.is_running(),
            "a stopped writer must not be reused as a live accessory"
        );
        drop(host);
    }

    #[test]
    fn challenge_echo_failure_marks_the_physical_owner_unavailable() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, mut host) = UnixStream::pair().unwrap();
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let key = crate::media_crypto::test_media_key(2);
        let _lease = bridge.prepare_media(key).unwrap();
        viewer.shutdown(std::net::Shutdown::Write).unwrap();
        let host_tx = secure_channel::DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        host.write_all(
            &usb_mux::encode(
                usb_mux::CHANNEL_MEDIA,
                &host_tx.seal(b"LCH1failed-echo").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while bridge.is_running() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !bridge.is_running(),
            "a failed challenge write must retire its physical reader"
        );
        drop(host);
    }

    #[test]
    fn control_plane_survives_logical_media_cancellation() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (viewer, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let bridge = Arc::new(UsbBridge::start(viewer.as_raw_fd()).unwrap());
        let lease = bridge
            .prepare_media(crate::media_crypto::test_media_key(7))
            .unwrap();
        drop(lease);
        let mut client = TcpStream::connect(("127.0.0.1", bridge.control_port())).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        client.write_all(b"catalog-request\n").unwrap();
        let mut header = [0u8; 5];
        host.read_exact(&mut header).unwrap();
        let mut payload = vec![0u8; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize];
        host.read_exact(&mut payload).unwrap();
        assert_eq!(payload, b"catalog-request");
        host.write_all(&usb_mux::encode(usb_mux::CHANNEL_CONTROL, b"catalog-response\n").unwrap())
            .unwrap();
        let mut response = [0u8; 17];
        client.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"catalog-response\n");
        bridge.stop_after_detach();
        drop(host);
    }

    #[test]
    fn aoap_media_memory_is_bounded_for_4k() {
        assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
        assert_eq!(MEDIA_CHANNEL_CAPACITY, 2);
        const { assert!(MAX_FRAME_BYTES * (MEDIA_CHANNEL_CAPACITY + 1) <= 48 * 1024 * 1024) };
    }

    #[test]
    fn start_rejects_invalid_fd() {
        assert!(UsbBridge::start(-1).is_err());
    }

    fn l2_frame(body_len: usize) -> Vec<u8> {
        let mut frame = Vec::with_capacity(21 + body_len);
        frame.push(b'G');
        frame.extend_from_slice(&[0x34, 0x12]); // AU id LE
        frame.extend_from_slice(b"L2");
        frame.extend_from_slice(&[0u8; 16]); // capture/encode wall clocks
        frame.extend(std::iter::repeat_n(0xABu8, body_len));
        frame
    }

    #[test]
    fn fragment_au_splits_large_frame_into_shim_datagrams() {
        let payload = l2_frame(3_000);
        let datagrams = fragment_au_frame(&payload).expect("L2 frame fragments");
        assert!(datagrams.len() >= 2);
        for (index, datagram) in datagrams.iter().enumerate() {
            assert_eq!(datagram[0], b'G');
            assert!(datagram.len() <= FRAGMENT_DATAGRAM_BYTES);
            assert_eq!(&datagram[1..3], &(index as u16).to_be_bytes());
            assert_eq!(&datagram[3..5], &(datagrams.len() as u16).to_be_bytes());
            // L2 header minus marker is preserved verbatim.
            assert_eq!(&datagram[5..25], &payload[1..21]);
        }
        // Reassembled payload matches the original body order.
        let reassembled: Vec<u8> = datagrams
            .iter()
            .flat_map(|datagram| datagram[FRAGMENT_HEADER_BYTES..].to_vec())
            .collect();
        assert_eq!(reassembled, payload[21..]);
    }

    #[test]
    fn fragment_au_passes_through_non_l2_payloads() {
        assert!(fragment_au_frame(b"CFG-rest").is_none());
        assert!(fragment_au_frame(b"LCH1-token").is_none());
        let mut short = vec![b'G'];
        short.extend_from_slice(b"L2");
        assert!(fragment_au_frame(&short).is_none());
    }
}
