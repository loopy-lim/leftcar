//! Android→Host KM TCP duplex owned by Rust (`secure_channel` + length-prefix framing).
//!
//! Mirrors Host `apps/host-desktop/src-tauri/src/km.rs` read_frame/write_frame:
//! big-endian u32 length + ChaCha20-Poly1305 stream frame. Kotlin must not open
//! sockets or touch AEAD keys.

use secure_channel::{km_keys, OpenError, StreamSealer, COUNTER_LEN, TAG_LEN};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Matches `control_contract::km::MAX_KM_PLAINTEXT` and Host `MAX_FRAME` bound.
const MAX_PLAINTEXT: usize = 8192;
const MAX_FRAME: usize = MAX_PLAINTEXT + COUNTER_LEN + TAG_LEN;
const MIN_FRAME: usize = COUNTER_LEN + TAG_LEN; // 24; Kotlin historically required 25 (counter+tag+≥1)
const QUEUE_CAP: usize = 256;

pub const KM_OK: i32 = 0;
pub const KM_ERR: i32 = 1;
pub const KM_ERR_CLOSED: i32 = 2;
pub const KM_ERR_TIMEOUT: i32 = 3;
pub const KM_ERR_TOO_LARGE: i32 = 4;
/// Distinct from [KM_ERR_TIMEOUT] so JNI panic unwind is not mistaken for idle timeout.
pub const KM_ERR_INTERNAL: i32 = 5;

struct OutboundQueue {
    items: Mutex<VecDeque<Vec<u8>>>,
    signal: Condvar,
}

impl OutboundQueue {
    fn new() -> Self {
        Self {
            items: Mutex::new(VecDeque::new()),
            signal: Condvar::new(),
        }
    }

    fn push(&self, plain: Vec<u8>) -> Result<(), i32> {
        let mut guard = self.items.lock().map_err(|_| KM_ERR)?;
        if guard.len() >= QUEUE_CAP {
            return Err(KM_ERR);
        }
        guard.push_back(plain);
        self.signal.notify_one();
        Ok(())
    }

    fn pop_timeout(&self, timeout: Duration) -> Option<Vec<u8>> {
        let Ok(mut guard) = self.items.lock() else {
            return None;
        };
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(item) = guard.pop_front() {
                return Some(item);
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let (next, _) = self
                .signal
                .wait_timeout(guard, deadline.saturating_duration_since(now))
                .ok()?;
            guard = next;
        }
    }

    fn clear(&self) {
        if let Ok(mut guard) = self.items.lock() {
            guard.clear();
        }
        self.signal.notify_all();
    }
}

pub struct KmDuplex {
    stop: Arc<AtomicBool>,
    outbound: Arc<OutboundQueue>,
    inbound: Mutex<Receiver<Result<Vec<u8>, String>>>,
    writer: Option<JoinHandle<()>>,
    reader: Option<JoinHandle<()>>,
    stream: Arc<Mutex<TcpStream>>,
}

impl KmDuplex {
    pub fn connect(
        host: &str,
        port: u16,
        seed: &[u8; 32],
        session_id: &str,
    ) -> Result<Self, String> {
        if !(1..=65535).contains(&port) {
            return Err("invalid KM port".into());
        }
        if session_id.is_empty() || session_id.len() > 128 {
            return Err("invalid KM session id".into());
        }
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| "KM host resolve failed".to_string())?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(2000))
            .map_err(|e| e.to_string())?;
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(1000)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;

        let keys = km_keys(seed, session_id);
        let stop = Arc::new(AtomicBool::new(false));
        // Separate halves so the reader can block without stalling the writer.
        let writer_stream = stream.try_clone().map_err(|e| e.to_string())?;
        let reader_stream = stream;
        let shutdown = Arc::new(Mutex::new(
            writer_stream.try_clone().map_err(|e| e.to_string())?,
        ));
        let outbound = Arc::new(OutboundQueue::new());
        let (inbound_tx, inbound_rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(QUEUE_CAP);
        let writer_errors = inbound_tx.clone();

        let writer_stop = Arc::clone(&stop);
        let writer_outbound = Arc::clone(&outbound);
        let mut tx = StreamSealer::new(keys.c2s);
        let writer = thread::Builder::new()
            .name("km-tcp-writer".into())
            .spawn(move || {
                let mut writer_stream = writer_stream;
                while !writer_stop.load(Ordering::SeqCst) {
                    let Some(plain) = writer_outbound.pop_timeout(Duration::from_millis(100))
                    else {
                        continue;
                    };
                    if plain.is_empty() || plain.len() > MAX_PLAINTEXT {
                        let _ = writer_errors.send(Err("KM plaintext size invalid".into()));
                        break;
                    }
                    let frame = match tx.seal(&plain) {
                        Ok(f) => f,
                        Err(_) => {
                            let _ = writer_errors.send(Err("KM seal failed".into()));
                            break;
                        }
                    };
                    let write = writer_stream
                        .write_all(&(frame.len() as u32).to_be_bytes())
                        .and_then(|_| writer_stream.write_all(&frame))
                        .and_then(|_| writer_stream.flush());
                    if let Err(error) = write {
                        let _ = writer_errors.send(Err(error.to_string()));
                        break;
                    }
                }
                writer_stop.store(true, Ordering::SeqCst);
                let _ = writer_stream.shutdown(Shutdown::Both);
            })
            .map_err(|e| e.to_string())?;

        let reader_stop = Arc::clone(&stop);
        let mut rx = StreamSealer::new(keys.s2c);
        let reader_inbound = inbound_tx;
        let reader = thread::Builder::new()
            .name("km-tcp-reader".into())
            .spawn(move || {
                let mut reader_stream = reader_stream;
                let fail = |msg: String| {
                    let _ = reader_inbound.send(Err(msg));
                    reader_stop.store(true, Ordering::SeqCst);
                };
                while !reader_stop.load(Ordering::SeqCst) {
                    let length = match read_u32_be(&mut reader_stream) {
                        Ok(v) => v as usize,
                        Err(error) => {
                            if reader_stop.load(Ordering::SeqCst) {
                                break;
                            }
                            if error == "timeout" {
                                continue;
                            }
                            fail(error);
                            break;
                        }
                    };
                    if !(MIN_FRAME..=MAX_FRAME).contains(&length) {
                        fail("Invalid KM frame size".into());
                        break;
                    }
                    let mut frame = vec![0u8; length];
                    if let Err(error) = read_exact(&mut reader_stream, &mut frame) {
                        fail(error);
                        break;
                    }
                    match rx.open(&frame) {
                        Ok(plain) => {
                            if reader_inbound.send(Ok(plain)).is_err() {
                                break;
                            }
                        }
                        Err(OpenError::TooLarge) => {
                            fail("KM frame too large".into());
                            break;
                        }
                        Err(_) => {
                            fail("KM open failed".into());
                            break;
                        }
                    }
                }
                reader_stop.store(true, Ordering::SeqCst);
                let _ = reader_stream.shutdown(Shutdown::Both);
            })
            .map_err(|e| e.to_string())?;

        Ok(Self {
            stop,
            outbound,
            inbound: Mutex::new(inbound_rx),
            writer: Some(writer),
            reader: Some(reader),
            stream: shutdown,
        })
    }

    pub fn send(&self, plain: &[u8]) -> Result<(), i32> {
        if self.stop.load(Ordering::SeqCst) {
            return Err(KM_ERR_CLOSED);
        }
        if plain.is_empty() || plain.len() > MAX_PLAINTEXT {
            return Err(KM_ERR_TOO_LARGE);
        }
        self.outbound.push(plain.to_vec())
    }

    pub fn discard_queued(&self) {
        self.outbound.clear();
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Vec<u8>, i32> {
        let rx = self.inbound.lock().map_err(|_| KM_ERR)?;
        match rx.recv_timeout(timeout) {
            Ok(Ok(plain)) => Ok(plain),
            Ok(Err(_)) => Err(KM_ERR_CLOSED),
            Err(RecvTimeoutError::Timeout) => Err(KM_ERR_TIMEOUT),
            Err(RecvTimeoutError::Disconnected) => Err(KM_ERR_CLOSED),
        }
    }

    pub fn close(&mut self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self
            .stream
            .lock()
            .ok()
            .and_then(|s| s.shutdown(Shutdown::Both).ok());
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}

impl Drop for KmDuplex {
    fn drop(&mut self) {
        self.close();
    }
}

fn read_u32_be(stream: &mut TcpStream) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    read_exact(stream, &mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<(), String> {
    let mut read = 0;
    while read < buf.len() {
        match stream.read(&mut buf[read..]) {
            Ok(0) => return Err("KM socket closed".into()),
            Ok(n) => read += n,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                if read == 0 {
                    return Err("timeout".into());
                }
                continue;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

/// Registry entry. Arc keeps the cell alive across concurrent send/recv while close
/// removes the id and takes the duplex under the inner mutex (no raw-pointer UAF).
struct KmDuplexHandle {
    duplex: Mutex<Option<KmDuplex>>,
}

fn handle_table() -> &'static Mutex<HashMap<i64, Arc<KmDuplexHandle>>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, Arc<KmDuplexHandle>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_handle_id() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id > 0 {
        id
    } else {
        // Extremely unlikely wrap; keep JNI "0 means missing" convention.
        NEXT.store(2, Ordering::Relaxed);
        1
    }
}

pub fn km_duplex_into_handle(duplex: KmDuplex) -> i64 {
    let id = next_handle_id();
    let cell = Arc::new(KmDuplexHandle {
        duplex: Mutex::new(Some(duplex)),
    });
    if handle_table()
        .lock()
        .map(|mut t| t.insert(id, cell))
        .is_err()
    {
        return 0;
    }
    id
}

pub fn km_duplex_with<R>(handle: i64, f: impl FnOnce(&KmDuplex) -> R, closed: R) -> R {
    if handle == 0 {
        return closed;
    }
    let cell = match handle_table().lock() {
        Ok(table) => table.get(&handle).cloned(),
        Err(_) => None,
    };
    let Some(cell) = cell else {
        return closed;
    };
    let Ok(guard) = cell.duplex.lock() else {
        return closed;
    };
    match guard.as_ref() {
        Some(duplex) => f(duplex),
        None => closed,
    }
}

pub fn km_duplex_drop_handle(handle: i64) {
    if handle == 0 {
        return;
    }
    let cell = handle_table()
        .lock()
        .ok()
        .and_then(|mut table| table.remove(&handle));
    let Some(cell) = cell else {
        return;
    };
    let taken = cell.duplex.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut duplex) = taken {
        duplex.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn seed() -> [u8; 32] {
        std::array::from_fn(|i| i as u8)
    }

    fn read_exact_retry(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
        let mut read = 0;
        while read < buf.len() {
            match stream.read(&mut buf[read..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "eof",
                    ))
                }
                Ok(n) => read += n,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    #[test]
    fn native_socket_exchanges_independently_encrypted_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let keys = km_keys(&seed(), "km-socket-test");
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_nodelay(true).unwrap();
            // Blocking reads with a generous deadline — avoid WouldBlock from
            // nonblocking accept inheritance on some platforms.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut len = [0u8; 4];
            read_exact_retry(&mut socket, &mut len).expect("peer length");
            let n = u32::from_be_bytes(len) as usize;
            let mut frame = vec![0u8; n];
            read_exact_retry(&mut socket, &mut frame).expect("peer frame");
            let hello = StreamSealer::new(keys.c2s).open(&frame).unwrap();
            let hello = String::from_utf8(hello).unwrap();
            assert!(hello.contains("\"origin\":\"android\""));
            assert!(hello.contains("\"kind\":\"hello\""));
            let state = br#"{"version":1,"sessionId":"km-socket-test","epoch":1,"seq":1,"origin":"mac","payload":{"kind":"state","owner":"local"}}"#;
            let response = StreamSealer::new(keys.s2c).seal(state).unwrap();
            socket
                .write_all(&(response.len() as u32).to_be_bytes())
                .unwrap();
            socket.write_all(&response).unwrap();
            socket.flush().unwrap();
            thread::sleep(Duration::from_millis(200));
        });

        let mut client = KmDuplex::connect("127.0.0.1", port, &seed(), "km-socket-test").unwrap();
        let hello = br#"{"version":1,"sessionId":"km-socket-test","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}"#;
        client.send(hello).unwrap();
        let plain = client.recv_timeout(Duration::from_secs(2)).unwrap();
        let text = String::from_utf8(plain).unwrap();
        assert!(text.contains("\"kind\":\"state\""));
        assert!(text.contains("\"owner\":\"local\""));
        client.close();
        peer.join().unwrap();
    }

    #[test]
    fn oversized_length_prefix_terminates_without_allocating_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(&u32::MAX.to_be_bytes()).unwrap();
            socket.flush().unwrap();
            thread::sleep(Duration::from_millis(200));
        });
        let mut client = KmDuplex::connect("127.0.0.1", port, &seed(), "bounded").unwrap();
        let err = client.recv_timeout(Duration::from_secs(2)).unwrap_err();
        assert_eq!(err, KM_ERR_CLOSED);
        client.close();
        peer.join().unwrap();
    }
}
