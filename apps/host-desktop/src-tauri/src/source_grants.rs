//! Durable Host-owned display grants. Grants persist across Host restarts —
//! including unclean ones (2026-09-19 product decision: 한 번 승인된 기기는
//! 계속 사용한다). The dirty=true journal published at open remains as the
//! durability probe: a Host that cannot durably write state must not admit
//! sessions whose later approvals would be equally uncertain.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};

#[derive(Default)]
struct LeaseState {
    revoked: bool,
    in_flight: usize,
}
#[derive(Default)]
pub struct SourceLease {
    state: Mutex<LeaseState>,
    idle: Condvar,
}
impl SourceLease {
    pub fn begin(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.revoked {
            return false;
        }
        state.in_flight += 1;
        true
    }
    pub fn end(&self) {
        let mut state = self.state.lock().unwrap();
        state.in_flight -= 1;
        if state.in_flight == 0 {
            self.idle.notify_all();
        }
    }
    pub fn invalidate(&self) {
        self.state.lock().unwrap().revoked = true;
    }
    pub fn current(&self) -> bool {
        !self.state.lock().unwrap().revoked
    }
    pub fn wait_idle(&self) {
        let mut state = self.state.lock().unwrap();
        while state.in_flight != 0 {
            state = self.idle.wait(state).unwrap();
        }
    }
    pub fn enter(&self) -> Option<SourceOperation<'_>> {
        self.begin().then(|| SourceOperation(self))
    }
}
pub struct SourceOperation<'a>(&'a SourceLease);
impl Drop for SourceOperation<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}

#[derive(Clone)]
pub struct CaptureAccess {
    pub revision: u64,
    pub source_id: String,
    /// Host-derived device/credential lifetime; never a Viewer claim or IP.
    pub owner: String,
    pub lease: Arc<SourceLease>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantView {
    pub credential_id: String,
    /// Host-process view order; never part of remote authentication.
    pub state_revision: u64,
    pub source_ids: Vec<String>,
    pub revision: u64,
    pub review_required: bool,
    /// Standing remote-input approval for this device. Screen grants and the
    /// input approval are separate decisions: viewing never implies input.
    pub input: bool,
    pub persistence_error: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Record {
    source_ids: Vec<String>,
    revision: u64,
    reviewed: bool,
    /// 기기별 상시 원격 입력 승인. 화면 승인과 달리 기본값은 꺼짐이다.
    #[serde(default)]
    input: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    dirty: bool,
    devices: BTreeMap<String, Record>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            version: 1,
            dirty: true,
            devices: BTreeMap::new(),
        }
    }
}
pub trait JournalWriter: Send + Sync {
    fn replace(&self, path: &Path, body: &[u8]) -> Result<(), String>;
}
struct FileJournalWriter;
impl JournalWriter for FileJournalWriter {
    fn replace(&self, path: &Path, body: &[u8]) -> Result<(), String> {
        durable_replace(path, body)
    }
}
pub struct GrantStore {
    writer: Arc<dyn JournalWriter>,
    path: Option<PathBuf>,
    journal: Journal,
    leases: BTreeMap<String, Vec<Weak<SourceLease>>>,
    errors: BTreeMap<String, String>,
    closed: bool,
}
impl GrantStore {
    pub fn open(path: Option<PathBuf>) -> Result<Self, String> {
        Self::open_with_writer(path, Arc::new(FileJournalWriter))
    }
    pub fn open_with_writer(
        path: Option<PathBuf>,
        writer: Arc<dyn JournalWriter>,
    ) -> Result<Self, String> {
        let mut journal = match path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok())
        {
            Some(journal) if journal.version == 1 => journal,
            _ => Journal::default(),
        };
        // 승인(reviewed)은 호스트 재시작·비정상 종료와 무관하게 유지된다
        // (2026-09-19 제품 결정: 한 번 승인된 기기는 계속 사용). 재검토는
        // 기기 레코드가 없거나 Host UI에서 소스/입력을 바꿀 때만 다시 요구된다.
        // dirty 게시는 리셋용이 아니라 개시 시점 저장 가능성 확인이다 — 이
        // 쓰기가 실패하면 이후 승인 변경도 durable하지 않으므로 개봉 자체를
        // 실패시켰다(아래 write_journal).
        journal.dirty = true;
        // Failure here must prevent all admission, even if an old clean file
        // still exists. No effective permission has been published yet.
        if let Some(path) = &path {
            write_journal(writer.as_ref(), path, &journal)?;
        }
        Ok(Self {
            writer,
            path,
            journal,
            leases: BTreeMap::new(),
            errors: BTreeMap::new(),
            closed: false,
        })
    }
    pub fn view(&self, owner: &str) -> GrantView {
        let record = self.journal.devices.get(owner).cloned().unwrap_or_default();
        GrantView {
            credential_id: owner.to_owned(),
            state_revision: 0,
            source_ids: record.source_ids,
            revision: record.revision,
            review_required: !record.reviewed,
            input: record.input,
            persistence_error: self.errors.get(owner).cloned(),
        }
    }
    pub fn allows(&self, owner: &str, source: &str) -> bool {
        !self.closed
            && self
                .journal
                .devices
                .get(owner)
                .is_some_and(|r| r.reviewed && r.source_ids.iter().any(|id| id == source))
    }

    /// 상시 원격 입력 승인 여부. 승인은 호스트 재시작과 무관하게 유지된다
    /// (2026-09-19 제품 결정) — 거부(input=false)도 마찬가지로 유지된다.
    pub fn input_allowed(&self, owner: &str) -> bool {
        !self.closed
            && self
                .journal
                .devices
                .get(owner)
                .is_some_and(|r| r.reviewed && r.input)
    }
    pub fn access(&mut self, owner: &str, source: &str) -> Result<CaptureAccess, String> {
        if !self.allows(owner, source) {
            return Err(
                "source_access_denied: Host에서 이 기기의 화면 접근을 허용한 뒤 다시 시도하세요"
                    .into(),
            );
        }
        let lease = Arc::new(SourceLease::default());
        let leases = self.leases.entry(owner.into()).or_default();
        leases.retain(|lease| lease.strong_count() != 0);
        leases.push(Arc::downgrade(&lease));
        Ok(CaptureAccess {
            revision: self.view(owner).revision,
            source_id: source.into(),
            owner: owner.into(),
            lease,
        })
    }
    pub fn invalidate(&mut self, owner: &str) -> Vec<Arc<SourceLease>> {
        let leases: Vec<_> = self
            .leases
            .remove(owner)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|lease| lease.upgrade())
            .collect();
        for lease in &leases {
            lease.invalidate();
        }
        leases
    }
    /// Caller serializes with authentication. Invalidation never waits or
    /// invokes a backend under that lock; the local caller drains afterwards.
    pub fn update(
        &mut self,
        owner: &str,
        mut sources: Vec<String>,
    ) -> (Result<GrantView, String>, Vec<Arc<SourceLease>>) {
        if self.closed {
            return (Err("Host is shutting down".into()), vec![]);
        }
        sources.sort();
        sources.dedup();
        let old = self.journal.devices.get(owner).cloned().unwrap_or_default();
        let removed = old.source_ids.iter().any(|id| !sources.contains(id));
        let leases = self.invalidate(owner);
        let next = Record {
            source_ids: sources,
            revision: old.revision + 1,
            reviewed: true,
            // 화면 목록 편집은 별개 결정인 입력 승인을 건드리지 않는다.
            input: old.input,
        };
        self.journal.devices.insert(owner.into(), next);
        let result = self.path.as_ref().map_or(Ok(()), |path| {
            write_journal(self.writer.as_ref(), path, &self.journal)
        });
        if let Err(error) = result {
            // Retain desired removals for shutdown. Never activate an addition
            // or review whose durable commit was uncertain.
            if removed {
                self.journal.devices.get_mut(owner).unwrap().reviewed = false;
            } else {
                self.journal.devices.insert(owner.into(), old);
            }
            self.errors.insert(owner.into(), error.clone());
            (Err(format!("source_grant_persistence_failed: {error}; 접근은 차단되었습니다. Host에서 다시 확인하세요")), leases)
        } else {
            self.errors.remove(owner);
            (Ok(self.view(owner)), leases)
        }
    }
    pub fn fence(&mut self) -> Vec<Arc<SourceLease>> {
        self.closed = true;
        let owners: Vec<_> = self.leases.keys().cloned().collect();
        owners
            .iter()
            .flat_map(|owner| self.invalidate(owner))
            .collect()
    }

    /// 기기별 상시 원격 입력 승인을 저장한다. 소스 목록과 무관한 단독 결정이므로
    /// 라이브 캡처 리스를 무효화하지 않는다(세션 단위 적용은 호출자가 맡는다).
    /// 저장 실패 시 되돌려 실패를 반환한다 — 승인의 지속 여부가 불확실하면
    /// 성공으로 보고하지 않는다.
    pub fn set_input(&mut self, owner: &str, allowed: bool) -> Result<GrantView, String> {
        if self.closed {
            return Err("Host is shutting down".into());
        }
        let old = self.journal.devices.get(owner).cloned().unwrap_or_default();
        let next = Record {
            source_ids: old.source_ids.clone(),
            revision: old.revision + 1,
            reviewed: old.reviewed,
            input: allowed,
        };
        self.journal.devices.insert(owner.into(), next);
        if let Some(path) = self.path.as_ref() {
            if let Err(error) = write_journal(self.writer.as_ref(), path, &self.journal) {
                self.journal.devices.insert(owner.into(), old);
                self.errors.insert(owner.into(), error.clone());
                return Err(format!("source_grant_persistence_failed: {error}"));
            }
        }
        self.errors.remove(owner);
        Ok(self.view(owner))
    }
    /// Only after the admission fence, lease drain and backend retirement.
    pub fn finish_shutdown(&mut self) -> Result<(), String> {
        if !self.closed {
            return Err("shutdown admission fence missing".into());
        }
        self.journal.dirty = false;
        let result = self.path.as_ref().map_or(Ok(()), |path| {
            write_journal(self.writer.as_ref(), path, &self.journal)
        });
        if result.is_err() {
            self.journal.dirty = true;
        }
        result
    }
}

// A child launch can briefly inherit unrelated flock descriptions even with
// FD_CLOEXEC. Coordinate only tests that own/reopen profile locks or launch
// subprocess fixtures; production locking and other parallel tests are unchanged.
#[cfg(test)]
pub(crate) fn profile_process_fixture() -> std::sync::MutexGuard<'static, ()> {
    static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    FIXTURES.lock().unwrap()
}

/// Same inode remains locked for the full Host runtime, including shutdown.
/// Do not unlink this file: a second inode could admit a second live owner.
pub fn lock_profile(path: &Path) -> Result<std::fs::File, String> {
    std::fs::create_dir_all(path.parent().ok_or("missing profile directory")?)
        .map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Host profile lock unavailable: {e}"))?;
    file.try_lock()
        .map_err(|e| format!("This Host profile is already in use or cannot be locked: {e}"))?;
    Ok(file)
}
fn write_journal(writer: &dyn JournalWriter, path: &Path, journal: &Journal) -> Result<(), String> {
    writer.replace(
        path,
        &serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?,
    )
}
pub fn durable_replace(path: &Path, body: &[u8]) -> Result<(), String> {
    durable_replace_checked(path, body, |_| Ok(()))
}
fn durable_replace_checked(
    path: &Path,
    body: &[u8],
    checkpoint: impl Fn(u8) -> std::io::Result<()>,
) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("missing grant directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".source-grants-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        checkpoint(1)?;
        let mut file = options.open(&temporary)?;
        file.write_all(body)?;
        checkpoint(2)?;
        file.sync_all()?;
        drop(file);
        checkpoint(3)?;
        #[cfg(not(target_os = "windows"))]
        {
            std::fs::rename(&temporary, path)?;
            checkpoint(4)?;
            std::fs::File::open(parent)?.sync_all()?;
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                MoveFileExW(
                    windows::core::PCWSTR(from.as_ptr()),
                    windows::core::PCWSTR(to.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(std::io::Error::other)?;
            // MoveFileExW combines replacement and flushing. Keep the same
            // post-replacement fault boundary as the Unix writer.
            checkpoint(4)?;
        }
        checkpoint(5)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("grant storage write failed: {e}"))
}

// The v9 native contract consumes exactly one Arc even when start fails.
#[cfg(target_os = "macos")]
pub(crate) extern "C" fn native_begin(context: *mut std::ffi::c_void) -> i32 {
    if context.is_null() {
        return 0;
    }
    i32::from(unsafe { &*context.cast::<SourceLease>() }.begin())
}
#[cfg(target_os = "macos")]
pub(crate) extern "C" fn native_end(context: *mut std::ffi::c_void) {
    if !context.is_null() {
        unsafe { &*context.cast::<SourceLease>() }.end();
    }
}
#[cfg(target_os = "macos")]
pub(crate) extern "C" fn native_release(context: *mut std::ffi::c_void) {
    if !context.is_null() {
        drop(unsafe { Arc::from_raw(context.cast::<SourceLease>()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    struct FaultWriter(AtomicU8);
    impl JournalWriter for FaultWriter {
        fn replace(&self, path: &Path, body: &[u8]) -> Result<(), String> {
            durable_replace_checked(path, body, |stage| {
                if self.0.load(Ordering::SeqCst) == stage {
                    Err(std::io::Error::other(format!("injected stage {stage}")))
                } else {
                    Ok(())
                }
            })
        }
    }
    fn temp() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("leftcar-task10-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("source_grants.json");
        (root, path)
    }
    #[test]
    fn task10_restart_reuses_grants_even_after_unclean_exit() {
        let (root, path) = temp();
        let mut store = GrantStore::open(Some(path.clone())).unwrap();
        assert!(!store.allows("a", "display-a"));
        store.update("a", vec!["display-a".into()]).0.unwrap();
        assert!(!store.allows("b", "display-a"));
        store.fence();
        store.finish_shutdown().unwrap();
        drop(store);
        let store = GrantStore::open(Some(path.clone())).unwrap();
        assert!(store.allows("a", "display-a"));
        assert!(!store.allows("a-new-credential", "display-a"));
        drop(store);
        // 비정상 종료(finish_shutdown 없이 다시 열기)여도 승인은 유지된다 —
        // 2026-09-19 제품 결정: 한 번 승인된 기기는 계속 사용한다.
        let store = GrantStore::open(Some(path)).unwrap();
        assert!(store.allows("a", "display-a"));
        assert!(!store.view("a").review_required);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn task10_dirty_establishment_failure_at_every_stage_never_publishes_grants() {
        for stage in 1..=5 {
            let (root, path) = temp();
            let mut store = GrantStore::open(Some(path.clone())).unwrap();
            store.update("a", vec!["display-a".into()]).0.unwrap();
            store.fence();
            store.finish_shutdown().unwrap();
            assert!(GrantStore::open_with_writer(
                Some(path),
                Arc::new(FaultWriter(AtomicU8::new(stage)))
            )
            .is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn task10_failed_removal_crash_and_clean_shutdown_never_restore_old_grant() {
        for stage in 1..=5 {
            for clean_shutdown in [false, true] {
                let (root, path) = temp();
                let writer = Arc::new(FaultWriter(AtomicU8::new(0)));
                let mut store =
                    GrantStore::open_with_writer(Some(path.clone()), writer.clone()).unwrap();
                store.update("a", vec!["display-a".into()]).0.unwrap();
                let access = store.access("a", "display-a").unwrap();
                writer.0.store(stage, Ordering::SeqCst);
                assert!(store.update("a", vec![]).0.is_err());
                assert!(!access.lease.current());
                assert!(!store.allows("a", "display-a"));
                assert!(store.view("a").source_ids.is_empty());
                assert!(store.view("a").persistence_error.is_some());
                if clean_shutdown {
                    writer.0.store(0, Ordering::SeqCst);
                    store.fence();
                    store.finish_shutdown().unwrap();
                }
                drop(store);
                let restarted = GrantStore::open(Some(path.clone())).unwrap();
                // 새 정책에는 전면 리셋이 없다 — 크래시 후 결과는 "마지막으로
                // 내구화된 저널"이 무엇인지로 결정된다. 제거 쓰기가 실패한
                // 내구 지점에 따라 저널은 이전 승인(record 유지)이거나 이미
                // 부분 확정된 차단(sources 비움)이다. 둘 다 마지막 확정 상태를
                // 따른다. 정상 종료는 메모리 차단을 내구화하므로 거부 유지.
                let durable: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(&path).unwrap(),
                )
                .unwrap();
                let record = &durable["devices"]["a"];
                let durable_granted = record["reviewed"].as_bool().unwrap_or(false)
                    && record["source_ids"]
                        .as_array()
                        .is_some_and(|ids| !ids.is_empty());
                assert_eq!(restarted.allows("a", "display-a"), durable_granted);
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
    #[test]
    fn task10_failed_addition_never_activates_and_shutdown_failure_contains_latest_denial() {
        for stage in 1..=5 {
            let (root, path) = temp();
            let writer = Arc::new(FaultWriter(AtomicU8::new(0)));
            let mut store =
                GrantStore::open_with_writer(Some(path.clone()), writer.clone()).unwrap();
            writer.0.store(stage, Ordering::SeqCst);
            assert!(store.update("a", vec!["display-a".into()]).0.is_err());
            assert!(!store.allows("a", "display-a"));
            writer.0.store(0, Ordering::SeqCst);
            store.update("a", vec!["display-a".into()]).0.unwrap();
            store.update("a", vec![]).0.unwrap();
            store.fence();
            writer.0.store(stage, Ordering::SeqCst);
            assert!(store.finish_shutdown().is_err());
            drop(store);
            assert!(!GrantStore::open(Some(path))
                .unwrap()
                .allows("a", "display-a"));
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn task10_profile_lock_excludes_second_owner_without_removing_inode() {
        let _fixture = profile_process_fixture();
        let (root, _) = temp();
        let path = root.join(".source-grants.lock");
        let owner = lock_profile(&path).unwrap();
        assert!(lock_profile(&path).is_err());
        drop(owner);
        let next = lock_profile(&path).unwrap();
        assert!(path.is_file());
        drop(next);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn task10_revoke_fences_new_native_operations_and_drains_admitted_output() {
        let lease = Arc::new(SourceLease::default());
        let operation = lease.enter().unwrap();
        lease.invalidate();
        assert!(lease.enter().is_none());
        assert!(!lease.current());
        let (tx, rx) = std::sync::mpsc::channel();
        let other = lease.clone();
        let thread = std::thread::spawn(move || {
            other.wait_idle();
            tx.send(()).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        drop(operation);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
    }
    #[test]
    fn task10_profile_lock_excludes_another_process_and_releases_on_process_exit() {
        const CHILD: &str = "LEFTCAR_TASK10_LOCK_CHILD";
        if let Ok(path) = std::env::var(CHILD) {
            let result = lock_profile(Path::new(&path));
            assert_eq!(
                result.is_ok(),
                std::env::var("LEFTCAR_TASK10_LOCK_EXPECT").unwrap() == "allow"
            );
            return;
        }
        let _fixture = profile_process_fixture();
        let (root, _) = temp();
        let path = root.join(".source-grants.lock");
        let owner = lock_profile(&path).unwrap();
        let child = |expected| {
            std::process::Command::new(std::env::current_exe().unwrap()).arg("source_grants::tests::task10_profile_lock_excludes_another_process_and_releases_on_process_exit").arg("--exact").env(CHILD,&path).env("LEFTCAR_TASK10_LOCK_EXPECT",expected).output().unwrap()
        };
        assert!(child("deny").status.success());
        drop(owner);
        assert!(child("allow").status.success());
        assert!(lock_profile(&path).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn task10_missing_corrupt_unknown_version_and_unreadable_journals_never_restore_access() {
        for contents in [
            None,
            Some(b"not JSON".as_slice()),
            Some(br#"{"version":2,"dirty":false,"devices":{}}"#.as_slice()),
        ] {
            let (root, path) = temp();
            if let Some(contents) = contents {
                std::fs::write(&path, contents).unwrap();
            }
            let store = GrantStore::open(Some(path)).unwrap();
            assert!(!store.allows("a", "display-a"));
            assert!(store.view("a").review_required);
            std::fs::remove_dir_all(root).unwrap();
        }
        let (root, path) = temp();
        std::fs::create_dir(&path).unwrap();
        assert!(GrantStore::open(Some(path)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
