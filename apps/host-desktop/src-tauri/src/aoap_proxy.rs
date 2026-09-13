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
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAX_FRAME_BYTES: usize = usb_mux::MAX_FRAME_BYTES;
const READ_BUFFER_BYTES: usize = 16 * 1024;

#[path = "aoap_proxy_lifecycle.rs"]
mod lifecycle;
use lifecycle::{ProxyManager, StopOutcome};

static ACTIVE_PROXY: std::sync::OnceLock<ProxyManager> = std::sync::OnceLock::new();

pub fn start_media_proxy(port: u16) -> Result<(), String> {
    ACTIVE_PROXY.get_or_init(ProxyManager::default).start(
        move |stop| prepare_media_proxy(port, stop, crate::aoap::take_usb_media_channel),
        move |job| {
            thread::Builder::new()
                .name(format!("leftcar-usb-media-{port}"))
                .spawn(job)
        },
    )
}

fn prepare_media_proxy(
    port: u16,
    stop: Cancellation,
    acquire: impl FnOnce() -> Option<crate::aoap::UsbMediaChannel>,
) -> Result<lifecycle::ProxyJob, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("USB media proxy bind failed on {port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("USB media proxy configuration failed: {error}"))?;
    let channel = acquire().ok_or("USB accessory link is not ready")?;
    Ok(Box::new(move || {
        let _lease = channel.lease;
        accept_media_connection(listener, channel.sender, channel.receiver, &stop);
    }))
}

pub fn stop_media_proxy() {
    if ACTIVE_PROXY.get_or_init(ProxyManager::default).stop() == StopOutcome::Incomplete {
        eprintln!("USB media proxy is still stopping; worker ownership retained");
    }
}

fn accept_media_connection(
    listener: TcpListener,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &Cancellation,
) {
    let (stream, peer) = loop {
        if cancelled(stop) {
            return;
        }
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(IO_POLL);
            }
            Err(_) => return,
        }
    };
    if cancelled(stop) {
        return;
    }
    eprintln!("USB media proxy accepted capture shim from {peer}");
    if let Err(error) = bridge_media_stream(stream, sender, receiver, stop) {
        eprintln!("USB media proxy stopped: {error}");
    }
}

// All socket and queue waits have a polling bound. The manager retains ownership
// if the OS scheduler takes longer than its stop budget to finish these workers.
const IO_POLL: Duration = Duration::from_millis(10);

type Cancellation = Arc<std::sync::atomic::AtomicBool>;

fn cancelled(stop: &std::sync::atomic::AtomicBool) -> bool {
    stop.load(std::sync::atomic::Ordering::Acquire)
}

struct CancelOnExit(Cancellation);

impl Drop for CancelOnExit {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn write_frame(
    writer: &mut impl Write,
    bytes: &[u8],
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if cancelled(stop) {
            return Ok(());
        }
        match writer.write(&bytes[offset..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(size) => offset += size,
            Err(error) if retryable(&error) => thread::sleep(IO_POLL),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn send_media(
    sender: &SyncSender<usb_mux::MuxFrame>,
    mut frame: usb_mux::MuxFrame,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    loop {
        if cancelled(stop) {
            return Ok(());
        }
        match sender.try_send(frame) {
            Ok(()) => return Ok(()),
            Err(std::sync::mpsc::TrySendError::Full(pending)) => frame = pending,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "USB link closed"));
            }
        }
        thread::sleep(IO_POLL);
    }
}

fn bridge_media_stream(
    stream: TcpStream,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &Cancellation,
) -> io::Result<()> {
    bridge_media_stream_with_spawn(stream, sender, receiver, stop, |job| {
        thread::Builder::new()
            .name("leftcar-usb-media-reader".into())
            .spawn(job)
    })
}

type ReaderJob = Box<dyn FnOnce() -> io::Result<()> + Send>;

// Own the reader through both normal return and parent unwinding. A stalled
// child keeps its parent alive, so the manager's timeout retains the lease.
struct BridgeReader {
    stop: Cancellation,
    shutdown: TcpStream,
    handle: Option<thread::JoinHandle<io::Result<()>>>,
}

impl BridgeReader {
    fn finish(&mut self) -> io::Result<()> {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.shutdown.shutdown(Shutdown::Both);
        let Some(reader) = self.handle.take() else {
            return Ok(());
        };
        while !reader.is_finished() {
            thread::sleep(IO_POLL);
        }
        reader
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("USB media reader panicked")))
    }
}

impl Drop for BridgeReader {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn bridge_media_stream_with_spawn(
    mut stream: TcpStream,
    sender: SyncSender<usb_mux::MuxFrame>,
    receiver: Receiver<Vec<u8>>,
    stop: &Cancellation,
    spawn: impl FnOnce(ReaderJob) -> io::Result<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let _cancel_on_exit = CancelOnExit(stop.clone());
    // Nonblocking reads and writes avoid platform-dependent timeout semantics,
    // including accepted BSD sockets inheriting O_NONBLOCK from the listener.
    stream.set_nonblocking(true)?;
    let shutdown = stream.try_clone()?;
    let mut reader_stream = stream.try_clone()?;
    let reader_stop = stop.clone();
    let reader_exit = CancelOnExit(reader_stop.clone());
    let reader = spawn(Box::new(move || {
        let _cancel_on_exit = reader_exit;
        let mut decoder = LengthPrefixDecoder::default();
        let mut buffer = [0u8; READ_BUFFER_BYTES];
        while !cancelled(&reader_stop) {
            let size = match reader_stream.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(size) => size,
                Err(error) if retryable(&error) => {
                    thread::sleep(IO_POLL);
                    continue;
                }
                Err(error) => return Err(error),
            };
            for payload in decoder.feed(&buffer[..size])? {
                send_media(
                    &sender,
                    usb_mux::MuxFrame {
                        channel: usb_mux::CHANNEL_MEDIA,
                        payload,
                    },
                    &reader_stop,
                )?;
            }
        }
        Ok(())
    }))?;

    let mut reader = BridgeReader {
        stop: stop.clone(),
        shutdown,
        handle: Some(reader),
    };
    let write_result = (|| {
        while !cancelled(stop) {
            let payload = match receiver.recv_timeout(IO_POLL) {
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
            write_frame(&mut stream, &frame, stop)?;
        }
        Ok(())
    })();
    write_result.and(reader.finish())
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
    fn shim_eof_completes_bridge_with_upstream_still_connected() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let (outgoing, _usb_reader) = std::sync::mpsc::sync_channel(1);
        let (upstream, incoming) = std::sync::mpsc::sync_channel(1);
        let (done, completion) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let result = bridge_media_stream(stream, outgoing, incoming, &stop);
            done.send(result).unwrap();
        });
        shim.shutdown(Shutdown::Both).unwrap();
        let completed = completion.recv_timeout(Duration::from_secs(1));
        // Release the old implementation's stuck receive before asserting RED.
        drop(upstream);
        worker.join().unwrap();
        assert!(completed.is_ok(), "shim EOF stranded the outer bridge");
    }

    #[test]
    fn partial_writes_retry_without_repeating_frame_prefix() {
        struct PartialWriter {
            bytes: Vec<u8>,
            calls: usize,
        }
        impl Write for PartialWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.calls += 1;
                match self.calls {
                    2 => Err(io::ErrorKind::WouldBlock.into()),
                    4 => Err(io::ErrorKind::TimedOut.into()),
                    5 => Err(io::ErrorKind::Interrupted.into()),
                    _ => {
                        let size = bytes.len().min(2);
                        self.bytes.extend_from_slice(&bytes[..size]);
                        Ok(size)
                    }
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = PartialWriter {
            bytes: Vec::new(),
            calls: 0,
        };
        let stop = std::sync::atomic::AtomicBool::new(false);
        let result = write_frame(&mut writer, b"\0\0\0\x03abc", &stop);
        assert!(
            result.is_ok(),
            "partial frame failed on a retryable write: {result:?}"
        );
        assert_eq!(writer.bytes, b"\0\0\0\x03abc");
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

#[cfg(test)]
#[path = "aoap_proxy_tests.rs"]
mod lifecycle_integration_tests;
