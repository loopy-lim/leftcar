//! Production ControlServer callers with only capture/network I/O substituted.
use super::*;
use crate::backend::CaptureBackend;
use crate::source_grants::CaptureAccess;
use control_contract::host::DisplayInfo;
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::sync::{Arc, Condvar};

const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
struct CaptureIO {
    displays: Mutex<Vec<DisplayInfo>>,
    calls: Mutex<Vec<(u32, CaptureAccess)>>,
    next: AtomicU32,
    stopped: AtomicUsize,
    input: Mutex<Vec<(u32, bool)>>,
    live: Mutex<HashMap<u32, (CaptureAccess, bool)>>,
    fail_start_once: AtomicBool,
    fail_disable: AtomicBool,
    fail_stop: AtomicBool,
    pause: AtomicBool,
    pause_stats: AtomicBool,
    entered: AtomicBool,
    released: Mutex<bool>,
    wake: Condvar,
}
impl CaptureIO {
    fn new() -> Self {
        Self {
            displays: Mutex::new(vec![display(0, "a"), display(1, "b")]),
            calls: Mutex::new(vec![]),
            next: AtomicU32::new(1),
            stopped: AtomicUsize::new(0),
            input: Mutex::new(vec![]),
            live: Mutex::new(HashMap::new()),
            fail_start_once: AtomicBool::new(false),
            fail_disable: AtomicBool::new(false),
            fail_stop: AtomicBool::new(false),
            pause: AtomicBool::new(false),
            pause_stats: AtomicBool::new(false),
            entered: AtomicBool::new(false),
            released: Mutex::new(false),
            wake: Condvar::new(),
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
fn display(index: u32, id: &str) -> DisplayInfo {
    DisplayInfo {
        index,
        source_id: Some(format!("test:display:{id}")),
        name: format!("Display {id}"),
        width: 1920,
        height: 1080,
    }
}
impl CaptureBackend for CaptureIO {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Ok(self.displays.lock().unwrap().clone())
    }
    fn start(
        &self,
        index: u32,
        _: &str,
        _: u16,
        _: u32,
        _: u32,
        _: u32,
        _: &str,
        _: &str,
        _: &str,
        _: EncoderExperiment,
        _: &AppliedUdpStability,
        _: &[u8; 32],
        access: Option<&CaptureAccess>,
    ) -> Result<u32, String> {
        let access = access.ok_or("missing native authorization")?;
        self.calls.lock().unwrap().push((index, access.clone()));
        if self.pause.load(Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.wake.wait(released).unwrap();
            }
        }
        // Native construction/output boundary executes after delayed setup.
        let _operation = access
            .lease
            .enter()
            .ok_or("source authorization revoked before native capture")?;
        if self.fail_start_once.swap(false, Ordering::SeqCst) {
            return Err("injected replacement failure".into());
        }
        let handle = self.next.fetch_add(1, Ordering::SeqCst);
        self.live
            .lock()
            .unwrap()
            .insert(handle, (access.clone(), false));
        Ok(handle)
    }
    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        if !self.live.lock().unwrap().contains_key(&handle) {
            return Err("native handle retired".into());
        }
        if self.pause_stats.load(Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.wake.wait(released).unwrap();
            }
        }
        Ok(StatsInfo {
            state: "running".into(),
            first_send_ms: 1,
            ..StatsInfo::default()
        })
    }
    fn stop(&self, handle: u32) -> Result<(), String> {
        self.stopped.fetch_add(1, Ordering::SeqCst);
        if self.fail_stop.load(Ordering::SeqCst) {
            Err("injected native stop failure".into())
        } else {
            self.live
                .lock()
                .unwrap()
                .remove(&handle)
                .ok_or("native handle retired")?;
            Ok(())
        }
    }
    fn input_permission(&self) -> Result<bool, String> {
        Ok(true)
    }
    fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
        let mut live = self.live.lock().unwrap();
        let (_, current) = live.get_mut(&handle).ok_or("native input handle retired")?;
        if !enabled && self.fail_disable.load(Ordering::SeqCst) {
            return Err("native disable failed".into());
        }
        *current = enabled;
        self.input.lock().unwrap().push((handle, enabled));
        Ok(())
    }
}
fn fixture() -> (
    Arc<ControlServer>,
    Arc<CaptureIO>,
    crate::pairing::Authorization,
    crate::pairing::Authorization,
) {
    let pairing = Arc::new(crate::pairing::PairingServer::new(
        [0; 32],
        None,
        Box::new(crate::pairing::FileTokenStore::new(None)),
    ));
    let pair = |device| {
        let offer = pairing.begin_pairing("127.0.0.1", 7777);
        let token = pairing.pair_by_code(&offer.code, device, device).unwrap();
        pairing.authenticate(&token).unwrap()
    };
    let a = pair("a");
    let b = pair("b");
    let io = Arc::new(CaptureIO::new());
    (
        Arc::new(ControlServer::new(
            io.clone(),
            pairing,
            Arc::new(secure_channel::HostIdentity::generate()),
        )),
        io,
        a,
        b,
    )
}
async fn call(
    server: &ControlServer,
    auth: &crate::pairing::Authorization,
    command: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    server
        .dispatch_with_authorization(
            command,
            args,
            "127.0.0.1",
            Some(auth.device_id()),
            Some(auth),
        )
        .await
}
fn start(id: &str, port: u16) -> serde_json::Value {
    json!({"sourceId":format!("test:display:{id}"),"sourceIndex":0,"viewerPort":port,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":KEY})
}
async fn entered(io: &CaptureIO) {
    let end = Instant::now() + Duration::from_secs(2);
    while !io.entered.load(Ordering::SeqCst) {
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}
#[tokio::test]
async fn task10_new_devices_have_no_catalog_or_start_access_and_two_devices_are_isolated() {
    let (server, io, a, b) = fixture();
    assert_eq!(
        call(&server, &a, "getCatalog", json!({})).await["result"]["displays"],
        json!([])
    );
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        false
    );
    assert!(io.calls.lock().unwrap().is_empty());
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    server
        .set_source_grants("b", vec!["test:display:b".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "getCatalog", json!({})).await["result"]["displays"][0]["sourceId"],
        "test:display:a"
    );
    assert_eq!(
        call(&server, &b, "getCatalog", json!({})).await["result"]["displays"][0]["index"],
        1
    );
    assert_eq!(
        call(&server, &a, "startStream", start("b", 5001)).await["ok"],
        false
    );
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    assert_eq!(
        call(&server, &b, "startStream", start("b", 5002)).await["ok"],
        true
    );
    let calls = io.calls.lock().unwrap();
    assert_ne!(calls[0].1.owner, calls[1].1.owner);
    assert_eq!(calls[0].1.source_id, "test:display:a");
    assert_eq!(calls[1].1.source_id, "test:display:b");
    assert!(
        io.input.lock().unwrap().is_empty(),
        "view grants never enable input"
    );
}
#[tokio::test]
async fn task10_catalog_reorder_legacy_snapshot_disappearance_and_ambiguity_never_switch_identity()
{
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into(), "test:display:b".into()])
        .unwrap();
    call(&server, &a, "getCatalog", json!({})).await;
    *io.displays.lock().unwrap() = vec![display(0, "b"), display(1, "a")];
    let mut legacy = start("a", 5001);
    legacy.as_object_mut().unwrap().remove("sourceId");
    assert_eq!(call(&server, &a, "startStream", legacy).await["ok"], true);
    {
        let calls = io.calls.lock().unwrap();
        assert_eq!(calls[0].0, 1);
        assert_eq!(calls[0].1.source_id, "test:display:a");
    }
    *io.displays.lock().unwrap() = vec![display(0, "b")];
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5002)).await["ok"],
        false
    );
    *io.displays.lock().unwrap() = vec![display(0, "a"), display(1, "a")];
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5002)).await["ok"],
        false
    );
    assert_eq!(io.calls.lock().unwrap().len(), 1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_removal_during_native_start_prevents_capture_output_and_registration() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    io.pause.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        tokio::spawn(async move { call(&server, &a, "startStream", start("a", 5001)).await })
    };
    entered(&io).await;
    let lease = io.calls.lock().unwrap()[0].1.lease.clone();
    server.set_source_grants("a", vec![]).unwrap();
    assert!(!lease.current());
    io.release();
    assert_eq!(running.await.unwrap()["ok"], false);
    assert!(server.snapshot().sessions.is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_reconfigure_reads_current_input_approval_and_reauthorizes_source() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into(), "test:display:b".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    server.set_session_input(1, true).unwrap();
    io.pause.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        let auth = a.clone();
        tokio::spawn(async move {
            call(
                &server,
                &auth,
                "reconfigureStream",
                json!({"session":1,"width":1280,"height":720,"fps":60}),
            )
            .await
        })
    };
    entered(&io).await;
    server.set_session_input(1, false).unwrap();
    io.release();
    assert_eq!(running.await.unwrap()["ok"], true);
    assert!(!server.snapshot().sessions[0].input_enabled);
    server.set_session_input(1, true).unwrap();
    assert_eq!(
        call(
            &server,
            &a,
            "reconfigureStream",
            json!({"session":1,"sourceId":"test:display:b","width":1280,"height":720,"fps":60})
        )
        .await["ok"],
        true
    );
    assert!(!server.snapshot().sessions[0].input_enabled);
    let calls = io.calls.lock().unwrap();
    assert!(calls
        .iter()
        .all(|(_, access)| access.owner == calls[0].1.owner));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_shutdown_fences_pending_start_and_admin_before_clean_commit() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    io.pause.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        tokio::spawn(async move { call(&server, &a, "startStream", start("a", 5001)).await })
    };
    entered(&io).await;
    let shutdown = {
        let server = server.clone();
        std::thread::spawn(move || server.shutdown_source_access())
    };
    while server.source_operations.current() {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(server
        .set_source_grants("a", vec!["test:display:a".into()])
        .is_err());
    assert!(!shutdown.is_finished());
    io.release();
    assert_eq!(running.await.unwrap()["ok"], false);
    shutdown.join().unwrap().unwrap();
    assert!(server.snapshot().sessions.is_empty());
}

#[test]
fn task10_actual_profile_startup_clean_shutdown_and_failed_removal_preserve_credentials() {
    let _fixture = crate::source_grants::profile_process_fixture();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for clean_shutdown in [false, true] {
                let root = std::env::temp_dir()
                    .join(format!("leftcar-task10-product-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&root).unwrap();
                let boot = || {
                    let owner =
                        crate::source_grants::lock_profile(&root.join(".source-grants.lock"))
                            .unwrap();
                    let pairing = Arc::new(crate::pairing::PairingServer::new(
                        [0; 32],
                        Some(root.join("paired_devices.json")),
                        Box::new(crate::pairing::FileTokenStore::new(Some(
                            root.join("tokens.json"),
                        ))),
                    ));
                    pairing
                        .initialize_source_grants(root.join("source_grants.json"))
                        .unwrap();
                    let io = Arc::new(CaptureIO::new());
                    let server = Arc::new(ControlServer::new(
                        io.clone(),
                        pairing.clone(),
                        Arc::new(secure_channel::HostIdentity::from_seed([1; 32])),
                    ));
                    (owner, pairing, server, io)
                };
                let (owner, pairing, server, _) = boot();
                let offer = pairing.begin_pairing("127.0.0.1", 7777);
                let token = pairing.pair_by_code(&offer.code, "a", "A").unwrap();
                server
                    .set_source_grants("a", vec!["test:display:a".into()])
                    .unwrap();
                server.shutdown_source_access().unwrap();
                drop(server);
                drop(pairing);
                drop(owner);
                let (owner, pairing, server, io) = boot();
                let auth = pairing.authenticate(&token).unwrap();
                assert_eq!(
                    call(&server, &auth, "getCatalog", json!({})).await["result"]["displays"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    call(&server, &auth, "startStream", start("a", 5001)).await["ok"],
                    true
                );
                // Actual failed replacement on the profile journal; credential files
                // stay intact. Restore the old dirty journal to model process restart.
                std::fs::rename(root.join("source_grants.json"), root.join("old-dirty.json"))
                    .unwrap();
                std::fs::create_dir(root.join("source_grants.json")).unwrap();
                assert!(server.set_source_grants("a", vec![]).is_err());
                assert!(server.snapshot().sessions.is_empty());
                assert_eq!(io.stopped.load(Ordering::SeqCst), 1);
                assert!(pairing.authenticate(&token).is_some());
                std::fs::remove_dir(root.join("source_grants.json")).unwrap();
                std::fs::rename(root.join("old-dirty.json"), root.join("source_grants.json"))
                    .unwrap();
                if clean_shutdown {
                    server.shutdown_source_access().unwrap();
                }
                drop(server);
                drop(pairing);
                drop(owner);
                let (owner, pairing, server, _) = boot();
                let auth = pairing.authenticate(&token).unwrap();
                assert_eq!(
                    call(&server, &auth, "getCatalog", json!({})).await["result"]["displays"],
                    json!([])
                );
                assert_eq!(
                    call(&server, &auth, "startStream", start("a", 5001)).await["ok"],
                    false
                );
                drop(server);
                drop(pairing);
                drop(owner);
                std::fs::remove_dir_all(root).unwrap();
            }
        });
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_removal_during_reconfigure_prevents_replacement_and_rollback() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into(), "test:display:b".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    io.pause.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        tokio::spawn(async move {
            call(&server,&a,"reconfigureStream",json!({"session":1,"sourceId":"test:display:b","sourceIndex":1,"width":1280,"height":720,"fps":60})).await
        })
    };
    entered(&io).await;
    server.set_source_grants("a", vec![]).unwrap();
    io.release();
    assert_eq!(running.await.unwrap()["ok"], false);
    assert!(server.snapshot().sessions.is_empty());
    assert!(io
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(_, access)| !access.lease.current()));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_force_stop_fences_pending_reconfigure_native_construction() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into(), "test:display:b".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    io.pause.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        tokio::spawn(async move {
            call(
                &server,
                &a,
                "reconfigureStream",
                json!({"session":1,"sourceId":"test:display:b","width":1280,"height":720,"fps":60}),
            )
            .await
        })
    };
    entered(&io).await;
    server.force_stop_session(1).unwrap();
    let pending_revoked = io
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(_, access)| !access.lease.current());
    io.release();
    assert_eq!(running.await.unwrap()["ok"], false);
    assert!(
        pending_revoked,
        "stopping a session must fence its pending replacement before native construction"
    );
}

#[tokio::test]
async fn task10_native_retirement_failure_is_visible_and_prevents_clean_shutdown_until_retried() {
    let (server, io, a, _) = fixture();
    let root = std::env::temp_dir().join(format!("leftcar-task10-stop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("source_grants.json");
    server
        .pairing
        .initialize_source_grants(path.clone())
        .unwrap();
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    io.fail_stop.store(true, Ordering::SeqCst);
    assert!(server
        .set_source_grants("a", vec![])
        .err()
        .unwrap()
        .contains("capture retirement"));
    assert!(server.shutdown_source_access().is_err());
    let read =
        || serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(read()["dirty"], true);
    io.fail_stop.store(false, Ordering::SeqCst);
    server.shutdown_source_access().unwrap();
    assert_eq!(read()["dirty"], false);
    assert!(io.stopped.load(Ordering::SeqCst) >= 3);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn task10_shutdown_waits_for_admitted_host_admin_write_before_final_clean_snapshot() {
    struct BlockWriter {
        block: AtomicBool,
        entered: AtomicBool,
        release: Mutex<bool>,
        wake: Condvar,
    }
    impl crate::source_grants::JournalWriter for BlockWriter {
        fn replace(&self, path: &std::path::Path, body: &[u8]) -> Result<(), String> {
            if self.block.load(Ordering::SeqCst) {
                self.entered.store(true, Ordering::SeqCst);
                let mut release = self.release.lock().unwrap();
                while !*release {
                    release = self.wake.wait(release).unwrap();
                }
            }
            crate::source_grants::durable_replace(path, body)
        }
    }
    let (server, _, _, _) = fixture();
    let root = std::env::temp_dir().join(format!("leftcar-task10-admin-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("source_grants.json");
    let writer = Arc::new(BlockWriter {
        block: AtomicBool::new(false),
        entered: AtomicBool::new(false),
        release: Mutex::new(false),
        wake: Condvar::new(),
    });
    server
        .pairing
        .initialize_source_grants_with_writer(path.clone(), writer.clone());
    writer.block.store(true, Ordering::SeqCst);
    let update = {
        let server = server.clone();
        std::thread::spawn(move || server.set_source_grants("a", vec!["test:display:a".into()]))
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !writer.entered.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let shutdown = {
        let server = server.clone();
        std::thread::spawn(move || server.shutdown_source_access())
    };
    while server.source_operations.current() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!shutdown.is_finished());
    assert!(server.set_source_grants("a", vec![]).is_err());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap()
            ["dirty"],
        true
    );
    *writer.release.lock().unwrap() = true;
    writer.wake.notify_all();
    update.join().unwrap().unwrap();
    shutdown.join().unwrap().unwrap();
    let disk = serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disk["dirty"], false);
    assert_eq!(
        disk["devices"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["source_ids"],
        json!(["test:display:a"])
    );
    assert!(server.set_source_grants("a", vec![]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_fix1_disable_after_native_retirement_controls_replacement_and_rollback() {
    for rollback in [false, true] {
        let (server, io, a, _) = fixture();
        server
            .set_source_grants("a", vec!["test:display:a".into()])
            .unwrap();
        assert_eq!(
            call(&server, &a, "startStream", start("a", 5001)).await["ok"],
            true
        );
        server.set_session_input(1, true).unwrap();
        io.pause.store(true, Ordering::SeqCst);
        io.fail_start_once.store(rollback, Ordering::SeqCst);
        let running = {
            let server = server.clone();
            tokio::spawn(async move {
                call(
                    &server,
                    &a,
                    "reconfigureStream",
                    json!({"session":1,"width":1280,"height":720,"fps":60}),
                )
                .await
            })
        };
        entered(&io).await;
        assert!(
            io.set_input_enabled(1, false).is_err(),
            "adapter must reject already-retired handles"
        );
        assert!(
            server.set_session_input(1, true).is_err(),
            "retired handle cannot be enabled"
        );
        let disabled = server.set_session_input(1, false);
        io.release();
        let result = running.await.unwrap();
        assert!(
            disabled.is_ok(),
            "logical disable during replacement: {disabled:?}"
        );
        assert_eq!(result["ok"], !rollback);
        assert!(!server.sessions.lock().unwrap().live[&1].input_enabled);
        assert!(
            io.live
                .lock()
                .unwrap()
                .values()
                .all(|(_, enabled)| !enabled),
            "replacement/rollback must never reenable the latest denied input"
        );
    }
}
#[tokio::test]
async fn task10_fix1_native_disable_failure_revokes_actual_authority_even_if_stop_fails() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    server.set_session_input(1, true).unwrap();
    io.fail_disable.store(true, Ordering::SeqCst);
    io.fail_stop.store(true, Ordering::SeqCst);
    assert!(server.set_session_input(1, false).is_err());
    assert!(!server.sessions.lock().unwrap().live[&1].input_enabled);
    let live = io.live.lock().unwrap();
    let (access, enabled) = &live[&1];
    assert!(*enabled, "native failure really retained the old flag");
    assert!(
        !access.lease.current(),
        "logical false cannot be the only fence when native disable and retirement fail"
    );
    drop(live);
    assert!(!server.pending_retirements.lock().unwrap().is_empty());
    io.fail_stop.store(false, Ordering::SeqCst);
    server.shutdown_source_access().unwrap();
    assert!(io.live.lock().unwrap().is_empty());
    assert!(server.pending_retirements.lock().unwrap().is_empty());
}

#[tokio::test]
async fn task10_fix1_actual_revoke_commands_surface_storage_errors_after_removal() {
    for all in [false, true] {
        let (server, io, a, b) = fixture();
        let root =
            std::env::temp_dir().join(format!("leftcar-fix1-revoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("source_grants.json");
        server
            .pairing
            .initialize_source_grants(path.clone())
            .unwrap();
        server
            .set_source_grants("a", vec!["test:display:a".into()])
            .unwrap();
        server
            .set_source_grants("b", vec!["test:display:b".into()])
            .unwrap();
        assert_eq!(
            call(&server, &a, "startStream", start("a", 5001)).await["ok"],
            true
        );
        if all {
            assert_eq!(
                call(&server, &b, "startStream", start("b", 5002)).await["ok"],
                true
            );
        }
        let owners: Vec<_> = io
            .live
            .lock()
            .unwrap()
            .values()
            .map(|(access, _)| (access.owner.clone(), access.source_id.clone()))
            .collect();
        std::fs::rename(&path, root.join("old.json")).unwrap();
        std::fs::create_dir(&path).unwrap();
        let result = if all {
            serde_json::to_value(server.revoke_all_devices()).unwrap()
        } else {
            serde_json::to_value(server.revoke_device("a")).unwrap()
        };
        assert!(!server.pairing.is_device_paired("a"));
        assert!(server.snapshot().sessions.is_empty());
        assert!(io.stopped.load(Ordering::SeqCst) > 0);
        assert!(result["persistenceErrors"].as_array().is_some_and(|errors|!errors.is_empty()),"actual administrative result must preserve storage failure after row removal: {result}");
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(root.join("old.json"), &path).unwrap();
        server.shutdown_source_access().unwrap();
        let restarted = crate::source_grants::GrantStore::open(Some(path)).unwrap();
        for (owner, source) in owners {
            assert!(!restarted.allows(&owner, &source));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn task10_fix1_host_view_order_and_expected_credential_reject_stale_approval_without_retiring_new_session(
) {
    let (server, io, _, _) = fixture();
    let before = server.pairing.list_device_state();
    let old = before
        .devices
        .iter()
        .find(|d| d.device_id == "a")
        .unwrap()
        .source_grants
        .credential_id
        .clone();
    let first = server
        .set_source_grants_for_credential("a", vec!["test:display:a".into()], Some(&old))
        .unwrap();
    assert!(first.state_revision > before.revision);
    let offer = server.pairing.begin_pairing("127.0.0.1", 7777);
    let token = server
        .pairing
        .pair_by_code(&offer.code, "a", "new a")
        .unwrap();
    let new = server.pairing.list_device_state();
    let current = &new
        .devices
        .iter()
        .find(|d| d.device_id == "a")
        .unwrap()
        .source_grants;
    assert!(new.revision > first.state_revision);
    assert_ne!(old, current.credential_id);
    let confirmed = server
        .set_source_grants_for_credential(
            "a",
            vec!["test:display:a".into()],
            Some(&current.credential_id),
        )
        .unwrap();
    let auth = server.pairing.authenticate(&token).unwrap();
    assert_eq!(
        call(&server, &auth, "startStream", start("a", 5001)).await["ok"],
        true
    );
    assert!(server
        .set_source_grants_for_credential("a", vec![], Some(&old))
        .is_err());
    assert_eq!(
        io.stopped.load(Ordering::SeqCst),
        0,
        "stale UI cannot retire the newly paired session"
    );
    assert_eq!(
        server.pairing.list_device_state().revision,
        confirmed.state_revision
    );
    let removal = server.revoke_device("a");
    assert!(removal.persistence_errors.is_empty());
    assert_eq!(removal.removed_devices.len(), 1);
    assert_eq!(
        removal.removed_devices[0].credential_id,
        current.credential_id
    );
    assert!(removal.state_revision > confirmed.state_revision);
    assert!(server
        .set_source_grants_for_credential("a", vec!["test:display:a".into()], Some(&old))
        .is_err());
    let all = server.revoke_all_devices();
    assert!(all.persistence_errors.is_empty());
    assert_eq!(all.removed_devices.len(), 1);
    let empty = server.pairing.list_device_state();
    assert_eq!(empty.revision, all.state_revision);
    assert!(empty.devices.is_empty());
    let no_op = server.revoke_device("missing");
    assert!(no_op.removed_devices.is_empty() && no_op.persistence_errors.is_empty());
    assert_eq!(no_op.state_revision, empty.revision);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn task10_fix1_reused_numeric_replacement_handle_is_not_old_retirement_proof() {
    let (server, io, a, _) = fixture();
    server
        .set_source_grants("a", vec!["test:display:a".into()])
        .unwrap();
    assert_eq!(
        call(&server, &a, "startStream", start("a", 5001)).await["ok"],
        true
    );
    io.next.store(1, Ordering::SeqCst); // native adapter may reuse a retired number
    io.pause_stats.store(true, Ordering::SeqCst);
    let running = {
        let server = server.clone();
        tokio::spawn(async move {
            call(
                &server,
                &a,
                "reconfigureStream",
                json!({"session":1,"width":1280,"height":720,"fps":60}),
            )
            .await
        })
    };
    entered(&io).await; // new native handle exists, but replacement is not committed
    assert_eq!(io.live.lock().unwrap().len(), 1);
    server.force_stop_session(1).unwrap();
    assert!(!io.live.lock().unwrap()[&1].0.lease.current());
    io.release();
    let result = running.await.unwrap();
    assert_eq!(result["ok"], false);
    assert!(
        io.live.lock().unwrap().is_empty(),
        "rejected new incarnation must actually retire despite numeric reuse"
    );
    assert_eq!(
        io.stopped.load(Ordering::SeqCst),
        2,
        "old and replacement incarnations each retire once"
    );
    assert!(server.pending_retirements.lock().unwrap().is_empty());
}
