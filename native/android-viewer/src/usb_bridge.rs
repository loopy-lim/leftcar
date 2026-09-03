//! Android UsbAccessory fd bridge.
//!
//! The renderer continues to use its existing UDP-side control socket and
//! media queue. This bridge replaces only the physical transport: channel 1
//! carries media/control datagrams and channel 0 carries the local JSON
//! control connection.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
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

pub struct UsbBridge {
    stop: Arc<AtomicBool>,
    control_addr: std::net::SocketAddr,
    control_port: u16,
    media_rx: Receiver<Vec<u8>>,
    worker: Option<JoinHandle<()>>,
}

impl UsbBridge {
    pub fn start(fd: i32) -> io::Result<Self> {
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
        let udp = UdpSocket::bind("127.0.0.1:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(50)))?;
        let control_addr = udp.local_addr()?;
        let control_port = listener.local_addr()?.port();
        let (media_tx, media_rx) = mpsc::sync_channel(MEDIA_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name(format!("leftcar-usb-bridge-{control_port}"))
            .spawn(move || {
                let reader_stop = Arc::clone(&worker_stop);
                let reader_writer = Arc::clone(&writer);
                let reader = thread::spawn(move || {
                    read_accessory(file, reader_writer, media_tx, control_tx, reader_stop);
                });
                let writer_stop = Arc::clone(&worker_stop);
                let udp_writer = Arc::clone(&writer);
                let writer_thread = thread::spawn(move || {
                    write_udp_media(udp, udp_writer, writer_stop);
                });
                run_control_proxy(listener, writer, control_rx, &worker_stop);
                worker_stop.store(true, Ordering::SeqCst);
                let _ = reader.join();
                let _ = writer_thread.join();
            })?;
        Ok(Self {
            stop,
            control_addr,
            control_port,
            media_rx,
            worker: Some(worker),
        })
    }

    pub fn control_addr(&self) -> std::net::SocketAddr {
        self.control_addr
    }

    pub fn control_port(&self) -> u16 {
        self.control_port
    }

    pub fn recv_media_timeout(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.media_rx.recv_timeout(timeout) {
            Ok(payload) => Ok(Some(payload)),
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

impl Drop for UsbBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The reader is intentionally not joined here: a blocking read on a
        // detached Android accessory may only wake after the OS closes fd.
        let _ = self.worker.take();
    }
}

fn read_accessory(
    mut file: File,
    writer: Arc<Mutex<File>>,
    media_tx: SyncSender<Vec<u8>>,
    control_tx: SyncSender<Vec<u8>>,
    stop: Arc<AtomicBool>,
) {
    let mut decoder = usb_mux::MuxDecoder::new();
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    while !stop.load(Ordering::SeqCst) {
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
            let target = match frame.channel {
                usb_mux::CHANNEL_CONTROL => &control_tx,
                usb_mux::CHANNEL_MEDIA => &media_tx,
                _ => unreachable!(),
            };
            if frame.channel == usb_mux::CHANNEL_MEDIA && frame.payload.starts_with(b"LCH1") {
                let Ok(bytes) = usb_mux::encode(usb_mux::CHANNEL_MEDIA, &frame.payload) else {
                    break;
                };
                let Ok(mut output) = writer.lock() else { break };
                if output.write_all(&bytes).is_err() {
                    break;
                }
                // Echo the challenge to the Host and also expose it to the
                // renderer so the authenticated IDR/input/feedback path can
                // use the same session token on USB as on UDP/TCP.
            }
            // Whole access units exceed the renderer's datagram buffers, so
            // they must re-enter it as shim-shaped fragments.
            if frame.channel == usb_mux::CHANNEL_MEDIA {
                if let Some(datagrams) = fragment_au_frame(&frame.payload) {
                    for datagram in datagrams {
                        if media_tx.send(datagram).is_err() {
                            return;
                        }
                    }
                    continue;
                }
            }
            if target.send(frame.payload).is_err() {
                return;
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
}

fn write_udp_media(udp: UdpSocket, writer: Arc<Mutex<File>>, stop: Arc<AtomicBool>) {
    let mut packet = vec![0u8; MAX_FRAME_BYTES.min(64 * 1024)];
    while !stop.load(Ordering::SeqCst) {
        let size = match udp.recv(&mut packet) {
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
        let Ok(bytes) = usb_mux::encode(usb_mux::CHANNEL_MEDIA, &packet[..size]) else {
            continue;
        };
        let Ok(mut output) = writer.lock() else { break };
        if output.write_all(&bytes).is_err() {
            break;
        }
    }
}

fn run_control_proxy(
    listener: TcpListener,
    writer: Arc<Mutex<File>>,
    control_rx: Receiver<Vec<u8>>,
    stop: &AtomicBool,
) {
    let mut stream: Option<TcpStream> = None;
    let mut input = Vec::new();
    let mut buffer = [0u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        if stream.is_none() {
            if let Ok((candidate, _)) = listener.accept() {
                let _ = candidate.set_nonblocking(true);
                stream = Some(candidate);
            }
        }
        if let Some(current) = stream.as_mut() {
            match current.read(&mut buffer) {
                Ok(0) => stream = None,
                Ok(size) => {
                    input.extend_from_slice(&buffer[..size]);
                    while let Some(newline) = input.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<u8> = input.drain(..=newline).collect();
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
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => stream = None,
            }
        }
        loop {
            match control_rx.try_recv() {
                Ok(payload) => {
                    if let Some(current) = stream.as_mut() {
                        if current.write_all(&payload).is_err() {
                            stream = None;
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
