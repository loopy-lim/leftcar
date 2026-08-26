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

const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;
const MEDIA_CHANNEL_CAPACITY: usize = 256;

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
    fn start_rejects_invalid_fd() {
        assert!(UsbBridge::start(-1).is_err());
    }
}
