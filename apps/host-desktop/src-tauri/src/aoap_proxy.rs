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
        return Err("USB media proxy is already active".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("USB media proxy bind failed on {port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("USB media proxy configuration failed: {error}"))?;
    let Some((sender, receiver)) = crate::aoap::take_usb_media_channel() else {
        return Err("USB accessory link is not ready".into());
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *active_proxy().lock().unwrap() = Some(stop.clone());
    thread::Builder::new()
        .name(format!("leftcar-usb-media-{port}"))
        .spawn(move || {
            accept_media_connection(listener, sender, receiver, &stop);
            crate::aoap::release_usb_media_channel();
            *active_proxy().lock().unwrap() = None;
        })
        .map_err(|error| format!("USB media proxy thread failed: {error}"))?;
    Ok(())
}

pub fn stop_media_proxy() {
    if let Some(stop) = active_proxy().lock().unwrap().as_ref() {
        stop.store(true, std::sync::atomic::Ordering::Release);
    }
    crate::aoap::release_usb_media_channel();
}

fn accept_media_connection(
    listener: TcpListener,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &std::sync::atomic::AtomicBool,
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
    eprintln!("USB media proxy accepted capture shim from {peer}");
    if let Err(error) = bridge_media_stream(stream, sender, receiver, stop) {
        eprintln!("USB media proxy stopped: {error}");
    }
}

fn bridge_media_stream(
    stream: TcpStream,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let writer = Arc::new(std::sync::Mutex::new(stream.try_clone()?));
    let reader_sender = sender;
    let reader_stream = stream;
    let reader = thread::spawn(move || -> io::Result<()> {
        let mut decoder = LengthPrefixDecoder::default();
        let mut buffer = [0u8; READ_BUFFER_BYTES];
        let mut stream = reader_stream;
        loop {
            let size = stream.read(&mut buffer)?;
            if size == 0 {
                return Ok(());
            }
            for payload in decoder.feed(&buffer[..size])? {
                reader_sender
                    .send(usb_mux::MuxFrame {
                        channel: usb_mux::CHANNEL_MEDIA,
                        payload,
                    })
                    .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "USB link closed"))?;
            }
        }
    });

    let mut write_result = Ok(());
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
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        let result = writer
            .lock()
            .map_err(|_| io::Error::other("USB media writer lock poisoned"))
            .and_then(|mut stream| stream.write_all(&frame));
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
