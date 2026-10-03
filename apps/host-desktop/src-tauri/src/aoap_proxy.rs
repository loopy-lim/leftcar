//! Loopback TCP proxy for the AOAP media channel.
//!
//! The macOS capture shim already has a bounded, authenticated TCP media
//! protocol. USB therefore only replaces the transport below that protocol:
//! this listener accepts the shim's framed stream and carries each payload as
//! one channel-1 mux frame. Viewer-to-host control datagrams use the reverse
//! direction of the same channel.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_FRAME_BYTES: usize = usb_mux::MAX_FRAME_BYTES;
const READ_BUFFER_BYTES: usize = 16 * 1024;

static ACTIVE_PROXY: std::sync::OnceLock<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>> =
    std::sync::OnceLock::new();

fn active_proxy() -> &'static Mutex<Option<Arc<std::sync::atomic::AtomicBool>>> {
    ACTIVE_PROXY.get_or_init(|| Mutex::new(None))
}

pub fn start_media_proxy(port: u16) -> Result<(), String> {
    if active_proxy().lock().unwrap().is_some() {
        // A previous session's proxy teardown is asynchronous: the stop flag
        // only breaks its accept/bridge loop on the next poll, so a
        // replacement stream started right after the old one can still see
        // the occupied slot. Stop it and wait briefly instead of failing —
        // the caller already tore down the predecessor transport.
        stop_media_proxy();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while active_proxy().lock().unwrap().is_some() {
            if std::time::Instant::now() >= deadline {
                return Err("USB media proxy is already active".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("USB media proxy bind failed on {port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("USB media proxy configuration failed: {error}"))?;
    let Some(channel) = crate::aoap::take_usb_media_channel() else {
        return Err("USB accessory link is not ready".into());
    };
    let sender = channel.sender;
    let receiver = channel.receiver;
    // MediaLease는 구독 식별자다 — 프록시 스레드 안에서 살아 있어야 하고,
    // 스레드가 끝나면 drop되어 미디어 구독이 해제된다(RAII 해제).
    let lease = channel.lease;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *active_proxy().lock().unwrap() = Some(stop.clone());
    thread::Builder::new()
        .name(format!("leftcar-usb-media-{port}"))
        .spawn(move || {
            let _lease = lease;
            accept_media_connection(listener, sender, receiver, &stop);
            *active_proxy().lock().unwrap() = None;
        })
        .map_err(|error| format!("USB media proxy thread failed: {error}"))?;
    Ok(())
}

pub fn stop_media_proxy() {
    if let Some(stop) = active_proxy().lock().unwrap().as_ref() {
        stop.store(true, std::sync::atomic::Ordering::Release);
    }
}

fn accept_media_connection(
    listener: TcpListener,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
) {
    let (stream, peer) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    };
    // BSD/macOS accepted sockets inherit the listener's O_NONBLOCK. The
    // dedicated reader thread must block: a WouldBlock read is a fatal
    // error there and would silently strand every shim media frame in the
    // kernel receive buffer after the first datagram.
    if let Err(error) = stream.set_nonblocking(false) {
        eprintln!("USB media proxy stream reset failed: {error}");
        return;
    }
    eprintln!("USB media proxy accepted capture shim from {peer}");
    if let Err(error) = bridge_media_stream(stream, sender, receiver, stop) {
        eprintln!("USB media proxy stopped: {error}");
    }
}

fn bridge_media_stream(
    stream: TcpStream,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    let writer = Arc::new(std::sync::Mutex::new(stream.try_clone()?));
    let reader_sender = sender;
    let reader_stream = stream;
    let reader_stop = Arc::clone(stop);
    let reader = thread::spawn(move || -> io::Result<()> {
        let mut decoder = LengthPrefixDecoder::default();
        let mut buffer = [0u8; READ_BUFFER_BYTES];
        let mut stream = reader_stream;
        let mut forwarded = 0usize;
        loop {
            if reader_stop.load(std::sync::atomic::Ordering::Acquire) {
                return Ok(());
            }
            let size = match stream.read(&mut buffer) {
                Ok(size) => size,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            };
            if size == 0 {
                eprintln!("USB media proxy shim->viewer EOF after {forwarded} frames");
                return Ok(());
            }
            for payload in decoder.feed(&buffer[..size])? {
                forwarded += 1;
                if forwarded <= 8 || forwarded.is_multiple_of(500) {
                    eprintln!(
                        "USB media proxy shim->viewer frame #{forwarded}: {}B head={:02x?}",
                        payload.len(),
                        &payload[..payload.len().min(8)]
                    );
                }
                send_usb_media_until_stopped(&reader_sender, payload, &reader_stop)?;
            }
        }
    });

    let mut write_result = Ok(());
    let mut upstream = 0usize;
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let payload = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(payload) => payload,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
            continue;
        }
        upstream += 1;
        if upstream <= 12 || upstream.is_multiple_of(200) {
            eprintln!(
                "USB media proxy viewer->shim frame #{upstream}: {}B head={:02x?}",
                payload.len(),
                &payload[..payload.len().min(8)]
            );
        }
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        let result = writer
            .lock()
            .map_err(|_| io::Error::other("USB media writer lock poisoned"))
            .and_then(|mut stream| write_capture_frame(&mut stream, &frame, stop));
        if let Err(error) = result {
            write_result = Err(error);
            break;
        }
    }
    if let Ok(stream) = writer.lock() {
        let _ = stream.shutdown(Shutdown::Both);
    }
    let _ = reader.join();
    write_result
}

fn send_usb_media_until_stopped(
    sender: &SyncSender<usb_mux::MuxFrame>,
    payload: Vec<u8>,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let mut frame = usb_mux::MuxFrame {
        channel: usb_mux::CHANNEL_MEDIA,
        payload,
    };
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        match sender.try_send(frame) {
            Ok(()) => return Ok(()),
            Err(std::sync::mpsc::TrySendError::Full(pending)) => {
                frame = pending;
                thread::sleep(Duration::from_millis(10));
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "USB link closed"));
            }
        }
    }
    Ok(())
}

/// The whole frame has one deadline; write timeouts alone would reset on
/// each partial write and let a slow capture peer retain the proxy forever.
fn write_capture_frame(
    stream: &mut TcpStream,
    frame: &[u8],
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut offset = 0;
    while offset < frame.len() {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "USB media proxy stopped",
            ));
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "capture peer stopped reading",
            ));
        }
        match stream.write(&frame[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "capture peer closed",
                ))
            }
            Ok(written) => offset += written,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Default)]
struct LengthPrefixDecoder {
    buffer: Vec<u8>,
}

impl LengthPrefixDecoder {
    fn feed(&mut self, bytes: &[u8]) -> io::Result<Vec<Vec<u8>>> {
        self.buffer.extend_from_slice(bytes);
        let mut payloads = Vec::new();
        loop {
            if self.buffer.len() < 4 {
                return Ok(payloads);
            }
            let length = u32::from_be_bytes(self.buffer[..4].try_into().unwrap()) as usize;
            if length == 0 || length > MAX_FRAME_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid USB media frame length",
                ));
            }
            if self.buffer.len() < 4 + length {
                return Ok(payloads);
            }
            payloads.push(self.buffer[4..4 + length].to_vec());
            self.buffer.drain(..4 + length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_releases_a_proxy_while_capture_is_not_reading_feedback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut capture = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        capture
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (stream, _) = listener.accept().unwrap();
        let (usb_tx, _usb_rx) = std::sync::mpsc::sync_channel(1);
        let (feedback_tx, feedback_rx) = std::sync::mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done, completed) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = bridge_media_stream(stream, usb_tx, feedback_rx, &worker_stop);
            let _ = done.send(result);
        });
        feedback_tx.send(vec![42; MAX_FRAME_BYTES]).unwrap();
        let mut prefix = [0u8; 5];
        capture.read_exact(&mut prefix).unwrap();
        // Writing has begun, but the rest exceeds the socket's send buffer.
        // The capture peer now deliberately stops reading.
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            completed.recv_timeout(Duration::from_secs(2)).is_ok(),
            "a blocked capture write must observe proxy cancellation"
        );
    }

    #[test]
    fn stop_releases_a_proxy_blocked_by_usb_backpressure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut capture = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let (usb_tx, usb_rx) = std::sync::mpsc::sync_channel(1);
        usb_tx
            .send(usb_mux::MuxFrame {
                channel: usb_mux::CHANNEL_MEDIA,
                payload: b"occupied".to_vec(),
            })
            .unwrap();
        let (feedback_tx, feedback_rx) = std::sync::mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done, completed) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = bridge_media_stream(stream, usb_tx, feedback_rx, &worker_stop);
            let _ = done.send(result);
        });
        capture.write_all(&[0, 0, 0, 1, 42]).unwrap();
        // Reader progress is observable through the original socket: it has
        // consumed our complete frame before the stop request.
        thread::sleep(Duration::from_millis(50));
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            completed.recv_timeout(Duration::from_secs(2)).is_ok(),
            "USB queue backpressure must not pin a stopped media proxy"
        );
        drop((usb_rx, feedback_tx));
    }

    #[test]
    fn length_prefix_decoder_roundtrip() {
        assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
        let mut decoder = LengthPrefixDecoder::default();
        let mut wire = Vec::new();
        wire.extend_from_slice(&4u32.to_be_bytes());
        wire.extend_from_slice(b"LCH1");
        wire.extend_from_slice(&3u32.to_be_bytes());
        wire.extend_from_slice(b"abc");
        assert_eq!(
            decoder.feed(&wire).unwrap(),
            vec![b"LCH1".to_vec(), b"abc".to_vec()]
        );
    }

    #[test]
    fn length_prefix_decoder_handles_split_input() {
        let mut decoder = LengthPrefixDecoder::default();
        assert!(decoder.feed(&[0, 0, 0]).unwrap().is_empty());
        assert_eq!(decoder.feed(&[3, b'a', b'b', b'c']).unwrap(), vec![b"abc"]);
    }

    #[test]
    fn length_prefix_decoder_rejects_invalid_length() {
        let mut decoder = LengthPrefixDecoder::default();
        assert!(decoder.feed(&[0, 0, 0, 0]).is_err());
        assert!(decoder.feed(&[0xff, 0, 0, 1]).is_err());
    }
}
