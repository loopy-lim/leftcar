//! Length-prefixed TCP media: idle ticks and incomplete frames are distinct.
use crate::input_gate::InputGate;
use std::io::{self, Read};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) const MAX_TCP_MEDIA_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) enum FrameRead {
    Idle,
    Closed,
    Frame(Vec<u8>),
}

pub(crate) fn read_frame(
    stream: &mut TcpStream,
    idle_timeout: Duration,
    frame_timeout: Duration,
) -> io::Result<FrameRead> {
    let old_timeout = stream.read_timeout()?;
    let result = read_frame_inner(stream, idle_timeout, frame_timeout);
    stream.set_read_timeout(old_timeout)?;
    result
}

fn read_frame_inner(
    stream: &mut TcpStream,
    idle_timeout: Duration,
    frame_timeout: Duration,
) -> io::Result<FrameRead> {
    let deadline = Instant::now() + frame_timeout;
    stream.set_read_timeout(Some(idle_timeout.min(frame_timeout)))?;
    let mut header = [0u8; 4];
    let received = match stream.read(&mut header) {
        Ok(0) => return Ok(FrameRead::Closed),
        Ok(received) => received,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(FrameRead::Idle)
        }
        Err(error) => return Err(error),
    };
    // Once any header byte has been consumed, this invocation owns that frame.
    // A timeout/EOF is fatal rather than an idle tick that discards alignment.
    read_until(stream, &mut header[received..], deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_TCP_MEDIA_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut payload = vec![0u8; length];
    read_until(stream, &mut payload, deadline)?;
    Ok(FrameRead::Frame(payload))
}

fn read_until(stream: &mut TcpStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(io::ErrorKind::TimedOut)?;
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(bytes) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(received) => bytes = &mut bytes[received..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(io::ErrorKind::TimedOut.into())
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn run_frames(
    stream: &mut TcpStream,
    stop: &AtomicBool,
    gate: &InputGate,
    mut on_frame: impl FnMut(&[u8]),
    mut on_idle: impl FnMut(),
    mut on_error: impl FnMut(io::Error),
) {
    while !stop.load(Ordering::Acquire) {
        let frame = read_frame(stream, Duration::from_millis(200), Duration::from_secs(2));
        if stop.load(Ordering::Acquire) {
            break;
        }
        match frame {
            Ok(FrameRead::Frame(packet)) => on_frame(&packet),
            Ok(FrameRead::Idle) => on_idle(),
            Ok(FrameRead::Closed) => break,
            Err(error) => {
                on_error(error);
                break;
            }
        }
    }
    stop.store(true, Ordering::Release);
    gate.retire();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Instant;

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (sender, listener.accept().unwrap().0)
    }
    #[test]
    fn fragmented_header_survives_an_idle_timeout_inside_a_frame() {
        let (mut sender, mut receiver) = pair();
        let writer = std::thread::spawn(move || {
            sender.write_all(&[0, 0]).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            sender.write_all(&[0, 3, 1, 2, 3]).unwrap();
        });
        let result = read_frame(
            &mut receiver,
            Duration::from_millis(20),
            Duration::from_millis(200),
        );
        writer.join().unwrap();
        assert_eq!(result.unwrap(), FrameRead::Frame(vec![1, 2, 3]));
    }
    #[test]
    fn whole_frame_deadline_bounds_a_continuously_trickling_payload() {
        let (mut sender, mut receiver) = pair();
        let writer = std::thread::spawn(move || {
            let mut framed = vec![0, 0, 0, 32];
            framed.extend_from_slice(&[7; 32]);
            for byte in framed {
                if sender.write_all(&[byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let started = Instant::now();
        let result = read_frame(
            &mut receiver,
            Duration::from_millis(30),
            Duration::from_millis(100),
        );
        let elapsed = started.elapsed();
        drop(receiver);
        writer.join().unwrap();
        assert!(
            result.is_err(),
            "partial-frame progress extended the total deadline: {result:?}"
        );
        assert!(
            elapsed < Duration::from_millis(250),
            "slow peer exceeded whole-frame budget: {elapsed:?}"
        );
    }
    #[test]
    fn idle_tick_and_clean_eof_are_not_partial_frame_failures() {
        let (sender, mut receiver) = pair();
        assert_eq!(
            read_frame(
                &mut receiver,
                Duration::from_millis(10),
                Duration::from_millis(50)
            )
            .unwrap(),
            FrameRead::Idle
        );
        drop(sender);
        assert_eq!(
            read_frame(
                &mut receiver,
                Duration::from_millis(10),
                Duration::from_millis(50)
            )
            .unwrap(),
            FrameRead::Closed
        );
    }
    #[test]
    fn eof_after_a_partial_header_is_fatal() {
        let (mut sender, mut receiver) = pair();
        sender.write_all(&[0, 0]).unwrap();
        drop(sender);
        assert_eq!(
            read_frame(
                &mut receiver,
                Duration::from_millis(10),
                Duration::from_millis(50)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    fn gate() -> (InputGate, std::net::UdpSocket) {
        use crate::media_sender::{MediaSender, MediaSocket};
        use std::sync::Arc;
        let receiver = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(receiver.local_addr().unwrap()).unwrap();
        let gate = InputGate::new(MediaSender::new(
            MediaSocket::Udp(Arc::new(socket)),
            &[97; 32],
        ));
        gate.set_enabled(true);
        receiver.recv(&mut [0; 128]).unwrap();
        (gate, receiver)
    }
    #[test]
    fn actual_input_loop_eof_and_partial_failure_retire_capture_and_input() {
        for partial in [false, true] {
            let (mut sender, mut receiver) = pair();
            if partial {
                sender.write_all(&[0, 0]).unwrap();
            }
            drop(sender);
            let stop = AtomicBool::new(false);
            let (gate, _status_receiver) = gate();
            let mut errors = 0;
            run_frames(
                &mut receiver,
                &stop,
                &gate,
                |_| panic!("EOF cannot inject input"),
                || {},
                |_| errors += 1,
            );
            assert!(
                stop.load(Ordering::Acquire),
                "input EOF/fatal did not stop its capture worker"
            );
            assert!(
                !gate.enabled(),
                "closed transport retained native input admission"
            );
            assert_eq!(errors, usize::from(partial));
        }
    }
    #[test]
    fn a_frame_returning_after_stop_never_runs_a_late_input_callback() {
        use std::sync::Arc;
        let (mut sender, mut receiver) = pair();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            stopped.store(true, Ordering::Release);
            sender.write_all(&[0, 0, 0, 1, 7]).unwrap();
        });
        let (gate, _status_receiver) = gate();
        let mut callbacks = 0;
        run_frames(
            &mut receiver,
            &stop,
            &gate,
            |_| callbacks += 1,
            || {},
            |_| {},
        );
        writer.join().unwrap();
        assert_eq!(
            callbacks, 0,
            "a retired stream applied a frame that arrived after stop"
        );
        assert!(!gate.enabled());
    }
}
