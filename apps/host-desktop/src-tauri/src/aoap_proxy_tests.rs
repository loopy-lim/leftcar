use super::*;
use crate::aoap::MediaSubscribers;
use std::sync::mpsc;
use std::time::Instant;

fn spawn(job: lifecycle::ProxyJob) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().spawn(job)
}

fn available_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_fake(
    manager: &ProxyManager,
    port: u16,
    slot: &MediaSubscribers,
    sender: SyncSender<usb_mux::MuxFrame>,
) -> Result<(), String> {
    manager.start(
        |stop| prepare_media_proxy(port, stop, || slot.acquire(sender)),
        spawn,
    )
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline, "watchdog expired");
        thread::yield_now();
    }
}

#[test]
fn accepting_stop_releases_same_port_and_subscriber_before_restart() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let port = available_port();
    start_fake(&manager, port, &slot, sender.clone()).unwrap();
    assert!(slot.acquire(sender.clone()).is_none());
    assert_eq!(manager.stop(), StopOutcome::Complete);
    start_fake(&manager, port, &slot, sender.clone()).unwrap();
    assert!(start_fake(&manager, available_port(), &slot, sender.clone()).is_err());
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn concurrent_different_ports_have_exactly_one_start_reservation() {
    let manager = Arc::new(ProxyManager::default());
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let (ready, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let owner = manager.clone();
    let first_slot = slot.clone();
    let first_sender = sender.clone();
    let first_port = available_port();
    let second_port = available_port();
    let first = thread::spawn(move || {
        owner.start(
            |stop| {
                let job =
                    prepare_media_proxy(first_port, stop, || first_slot.acquire(first_sender))?;
                ready.send(()).unwrap();
                wait.recv().unwrap();
                Ok(job)
            },
            spawn,
        )
    });
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(start_fake(&manager, second_port, &slot, sender).is_err());
    release.send(()).unwrap();
    assert!(first.join().unwrap().is_ok());
    assert_eq!(manager.stop(), StopOutcome::Complete);
}

#[test]
fn bind_channel_and_spawn_failures_release_lease_and_reservation() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = occupied.local_addr().unwrap().port();
    assert!(start_fake(&manager, port, &slot, sender.clone())
        .unwrap_err()
        .contains("bind failed"));
    drop(occupied);
    assert!(manager
        .start(|stop| prepare_media_proxy(port, stop, || None), spawn)
        .is_err());
    assert!(manager
        .start(
            |stop| prepare_media_proxy(port, stop, || slot.acquire(sender.clone())),
            |job| {
                drop(job);
                Err(io::Error::other("injected spawn failure"))
            }
        )
        .is_err());
    start_fake(&manager, port, &slot, sender).unwrap();
    assert_eq!(manager.stop(), StopOutcome::Complete);
}

#[test]
fn bidirectional_loopback_preserves_length_prefix_payload_and_fifo() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, usb) = mpsc::sync_channel(1);
    let port = available_port();
    start_fake(&manager, port, &slot, sender).unwrap();
    let mut shim = TcpStream::connect(("127.0.0.1", port)).unwrap();
    shim.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    shim.write_all(b"\0\0\0\x04LCH1\0\0\0\x03abc").unwrap();
    for expected in [b"LCH1".as_slice(), b"abc".as_slice()] {
        let frame = usb.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(frame.channel, usb_mux::CHANNEL_MEDIA);
        assert_eq!(frame.payload, expected);
    }
    slot.dispatch(b"xyz".to_vec());
    slot.dispatch(b"BYE".to_vec());
    let mut received = [0u8; 14];
    shim.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"\0\0\0\x03xyz\0\0\0\x03BYE");
    assert_eq!(manager.stop(), StopOutcome::Complete);
}

#[test]
fn eof_malformed_and_usb_disconnect_release_lease_and_allow_restart() {
    for cause in ["eof", "malformed", "usb"] {
        let manager = ProxyManager::default();
        let slot = MediaSubscribers::default();
        let (sender, usb) = mpsc::sync_channel(1);
        let port = available_port();
        start_fake(&manager, port, &slot, sender.clone()).unwrap();
        let mut shim = TcpStream::connect(("127.0.0.1", port)).unwrap();
        if cause == "eof" {
            shim.shutdown(Shutdown::Write).unwrap();
        } else if cause == "malformed" {
            shim.write_all(b"\0\0\0\0").unwrap();
        } else {
            drop(usb);
            shim.write_all(b"\0\0\0\x03abc").unwrap();
        }
        // Reacquisition proves worker RAII cleanup ran, independent of manager.
        wait_until(|| slot.acquire(sender.clone()).is_some());
        // Start automatically reaps the completed predecessor. Stop never runs
        // before this success, so it cannot mask natural-exit cleanup failures.
        wait_until(|| start_fake(&manager, port, &slot, sender.clone()).is_ok());
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }
}

#[test]
fn reader_spawn_failure_cancels_and_releases_lease() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let _shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let (done, completed) = mpsc::channel();
    let (inject, after_start) = mpsc::channel();
    let (at_gate, reached_gate) = mpsc::channel();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    at_gate.send(()).unwrap();
                    after_start.recv().unwrap();
                    let result = bridge_media_stream_with_spawn(
                        stream,
                        channel.sender,
                        channel.receiver,
                        &stop,
                        |job| {
                            drop(job);
                            Err(io::Error::other("injected reader spawn failure"))
                        },
                    );
                    assert!(cancelled(&stop));
                    done.send(result).unwrap();
                }))
            },
            |job| {
                let handle = spawn(job)?;
                // Force the worker to run before publication, while injection
                // remains gated until start has actually returned successfully.
                reached_gate.recv_timeout(Duration::from_secs(1)).unwrap();
                Ok(handle)
            },
        )
        .unwrap();
    inject.send(()).unwrap();
    assert!(completed
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn full_usb_queue_stop_joins_reader_while_receiver_remains_alive() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, usb) = mpsc::sync_channel(1);
    sender
        .send(usb_mux::MuxFrame {
            channel: usb_mux::CHANNEL_MEDIA,
            payload: vec![9],
        })
        .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let observer = stream.try_clone().unwrap();
    observer.set_nonblocking(true).unwrap();
    shim.write_all(b"\0\0\0\x03abc").unwrap();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    bridge_media_stream(stream, channel.sender, channel.receiver, &stop).unwrap();
                }))
            },
            spawn,
        )
        .unwrap();
    // The queue is full before the reader consumes the known socket frame.
    wait_until(
        || matches!(observer.peek(&mut [0u8; 1]), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
    );
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert_eq!(usb.recv().unwrap().payload, vec![9]);
    assert!(usb.try_recv().is_err());
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn nonreading_shim_cannot_strand_stop_in_a_partial_upstream_frame() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    shim.set_nonblocking(true).unwrap();
    let (stream, _) = listener.accept().unwrap();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    bridge_media_stream(stream, channel.sender, channel.receiver, &stop).unwrap();
                }))
            },
            spawn,
        )
        .unwrap();
    slot.dispatch(vec![7; MAX_FRAME_BYTES]);
    // Peek proves writing began without draining the peer's receive buffer.
    wait_until(|| matches!(shim.peek(&mut [0u8; 1]), Ok(size) if size > 0));
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn endless_retryable_writes_are_cancelled_and_write_zero_is_an_error() {
    struct RetryWriter(mpsc::Sender<()>);
    impl Write for RetryWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            let _ = self.0.send(());
            Err(io::ErrorKind::WouldBlock.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_stop = stop.clone();
    let (ready, retrying) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let worker = thread::spawn(move || {
        done.send(write_frame(
            &mut RetryWriter(ready),
            b"\0\0\0\x03abc",
            &worker_stop,
        ))
        .unwrap();
    });
    retrying.recv_timeout(Duration::from_secs(1)).unwrap();
    stop.store(true, std::sync::atomic::Ordering::Release);
    completed
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    stop.store(false, std::sync::atomic::Ordering::Release);
    let error = write_frame(&mut &mut [][..], b"abc", &stop).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}

#[test]
fn reader_panic_cancels_parent_reaps_child_and_releases_lease() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let _shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let (done, completed) = mpsc::channel();
    let (inject, after_start) = mpsc::channel();
    let (at_gate, reached_gate) = mpsc::channel();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    at_gate.send(()).unwrap();
                    after_start.recv().unwrap();
                    let result = bridge_media_stream_with_spawn(
                        stream,
                        channel.sender,
                        channel.receiver,
                        &stop,
                        |job| {
                            thread::Builder::new().spawn(move || {
                                let _job = job;
                                panic!("injected reader panic");
                            })
                        },
                    );
                    done.send(result).unwrap();
                }))
            },
            |job| {
                let handle = spawn(job)?;
                // Force the worker to run before publication, while injection
                // remains gated until start has actually returned successfully.
                reached_gate.recv_timeout(Duration::from_secs(1)).unwrap();
                Ok(handle)
            },
        )
        .unwrap();
    inject.send(()).unwrap();
    let result = completed.recv_timeout(Duration::from_secs(1));
    // Ensure RED cleans up the parent even before panic cancellation is fixed.
    let stopped = manager.stop();
    assert!(result.is_ok(), "reader panic stranded parent");
    assert!(result.unwrap().is_err());
    assert_eq!(stopped, StopOutcome::Complete);
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn upstream_disconnect_completes_parent_and_joins_idle_reader() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let _shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let (upstream, incoming) = mpsc::sync_channel(1);
    let (done, completed) = mpsc::channel();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    done.send(bridge_media_stream(stream, channel.sender, incoming, &stop))
                        .unwrap();
                }))
            },
            spawn,
        )
        .unwrap();
    drop(upstream);
    completed
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert!(slot.acquire(sender).is_some());
}

#[test]
fn parent_unwind_retains_lease_until_stalled_reader_is_reaped() {
    let manager = ProxyManager::default();
    let slot = MediaSubscribers::default();
    let (sender, _usb) = mpsc::sync_channel(1);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let _shim = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let (release, wait) = mpsc::channel();
    let (ready, unwinding) = mpsc::channel();
    let (done, child_done) = mpsc::channel();
    let (inject, after_start) = mpsc::channel();
    let (at_gate, reached_gate) = mpsc::channel();
    manager
        .start(
            |stop| {
                let channel = slot.acquire(sender.clone()).unwrap();
                Ok(Box::new(move || {
                    let _lease = channel.lease;
                    at_gate.send(()).unwrap();
                    after_start.recv().unwrap();
                    let child = thread::spawn(move || {
                        wait.recv().unwrap();
                        done.send(()).unwrap();
                        Ok(())
                    });
                    let _reader = BridgeReader {
                        stop,
                        shutdown: stream,
                        handle: Some(child),
                    };
                    ready.send(()).unwrap();
                    panic!("injected parent panic");
                }))
            },
            |job| {
                let handle = spawn(job)?;
                // Force the worker to run before publication, while injection
                // remains gated until start has actually returned successfully.
                reached_gate.recv_timeout(Duration::from_secs(1)).unwrap();
                Ok(handle)
            },
        )
        .unwrap();
    inject.send(()).unwrap();
    unwinding.recv_timeout(Duration::from_secs(1)).unwrap();
    let stopped = manager.stop();
    let lease_was_retained = slot.acquire(sender.clone()).is_none();
    release.send(()).unwrap();
    child_done.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(manager.stop(), StopOutcome::Complete);
    assert_eq!(
        stopped,
        StopOutcome::Incomplete,
        "parent detached its live reader"
    );
    assert!(
        lease_was_retained,
        "lease released before reader completion"
    );
    assert!(slot.acquire(sender).is_some());
}
