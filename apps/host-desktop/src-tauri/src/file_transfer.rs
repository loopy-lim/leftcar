//! 파일 전송 v1 — 암호화된 제어 채널 위의 청크 전송 상태.
//!
//! 방향은 둘 다 제어 명령으로 운반된다:
//! - 뷰어 → 호스트: `sendFileBegin/Chunk/End` (기기별 incoming 스테이징)
//! - 호스트 → 뷰어: 호스트 사용자가 고른 공유 대기열(`ShareQueueEntry`)을
//!   뷰어가 `fetchFileBegin/Chunk/End`로 당겨 간다.
//!
//! 파일 토큰은 프로세스 메모리에만 살고(재시작 시 무효), 스테이징 파일은
//! `~/Downloads/leftcar/<기기명>/` 아래 `.part`로만 존재하다가 완성 시
//! 최종 이름으로 바뀐다. 게이트(`settings.file_share`)는 모든 명령 앞에서
//! 검사하며 기본값은 꺼짐이다.

use base64::Engine as _;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 파일 상한(512 MiB) — v2부터 양방향 모두 청크 스트리밍이므로 메모리가
/// 아니라 디스크 기준으로만 산다.
pub const MAX_FILE_SIZE: u64 = 512 * 1024 * 1024;
/// 청크 1개(디코딩 후) 상한. fetch와 send가 같은 상한을 쓴다.
pub const MAX_CHUNK_BYTES: usize = 1024 * 1024;
/// 파일 토큰 바이트 수(16B → hex 32자).
const TOKEN_BYTES: usize = 16;
/// 버려진 전송(뷰어가 End/Cancel 없이 사라진 경우)의 만료 시간. 만료는
/// 마지막 활동(begin·청크) 기준 — created_at 기준이면 512MiB 전송이 30분을
/// 넘기는 순간 잘리므로, 활동이 이어지는 전송은 살아 있어야 한다. 만료된
/// 스테이징 `.part`와 상태 항목은 다음 begin 호출 때 치운다.
const STALE_TRANSFER: Duration = Duration::from_secs(30 * 60);
/// 동시 진행 incoming 상한. 게이트가 켜진 상태에서 무한 begin으로 디스크를
/// 채우는 것을 막는다(만료 스윕과 함께 이중 안전망). 상한 512 MiB와 곱해
/// 최악의 잔여 스테이징이 4 GiB를 넘지 않게 맞춘 값.
const MAX_CONCURRENT_INCOMING: usize = 8;

/// 공유 대기열 항목. `path`는 호스트 로컬 정보라서 직렬화에서 뺀다 — 뷰어와
/// UI에는 이름·크기·큐 ID만 보인다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareQueueEntry {
    pub queue_id: String,
    pub name: String,
    pub size: u64,
    #[serde(skip_serializing)]
    pub path: PathBuf,
}

struct IncomingTransfer {
    device: String,
    name: String,
    size: u64,
    written: u64,
    staging_path: PathBuf,
    last_activity: Instant,
}

struct OutgoingTransfer {
    device: String,
    name: String,
    path: PathBuf,
    size: u64,
    last_activity: Instant,
}

#[derive(Default)]
struct Inner {
    incoming: HashMap<String, IncomingTransfer>,
    outgoing: HashMap<String, OutgoingTransfer>,
    queue: Vec<ShareQueueEntry>,
}

trait OwnedTransfer {
    fn owned_by(&self, device: &str) -> bool;
}

impl OwnedTransfer for IncomingTransfer {
    fn owned_by(&self, device: &str) -> bool {
        self.device == device
    }
}

impl OwnedTransfer for OutgoingTransfer {
    fn owned_by(&self, device: &str) -> bool {
        self.device == device
    }
}

/// 소유 검사를 remove 앞에 둔다 — 먼저 지우고 걸러내면 다른 기기의 잘못된
/// End/Cancel이 실제 소유자의 전송 상태까지 파괴한다.
fn remove_owned<T: OwnedTransfer>(
    map: &mut HashMap<String, T>,
    token: &str,
    device: &str,
) -> Result<T, String> {
    if map.get(token).is_none_or(|t| !t.owned_by(device)) {
        return Err("unknown file token".into());
    }
    Ok(map.remove(token).expect("ownership checked above"))
}

/// 제어 평면 파일 전송의 공유 상태. ControlServer가 소유하고, Tauri UI 명령
/// (공유 대기열 관리)이 같은 인스턴스를 본다.
#[derive(Default)]
pub struct FileTransferState {
    inner: Mutex<Inner>,
    /// 테스트 주입용 루트. 없으면 `~/Downloads`를 쓰고, HOME을 모르면 오류.
    incoming_root: Mutex<Option<PathBuf>>,
}

/// device 토큰 문자열을 디렉터리 이름으로 바꾼다. 허용: [A-Za-z0-9-_ ],
/// 나머지는 '_'로 치환. 빈 결과는 "device".
pub fn sanitize_device_name(device: &str) -> String {
    let sanitized: String = device
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim().to_owned();
    if trimmed.is_empty() {
        "device".into()
    } else {
        trimmed
    }
}

/// Windows 예약 장치 이름. `CON.txt`처럼 확장자가 붙어도 정규화에서 장치로
/// 풀리므로 스템 기준으로 거부한다(Win32 경로 정규화 동작).
const WINDOWS_RESERVED_STEMS: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 업로드 파일명 검증. 경로 구분자·`..`·선행 점(숨김 파일)을 거부하고
/// 길이는 1..=255바이트로 제한한다. Windows 예약 장치 이름과 후행 점·공백도
/// 거부한다(교차 플랫폼 호스트를 위한 것 — macOS에선 무해하다).
/// Ok(())면 단일 파일명으로 안전하다.
pub fn validate_file_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("file name must not be empty".into());
    }
    if name.len() > 255 {
        return Err("file name is too long".into());
    }
    if name.starts_with('.') {
        return Err("file name must not start with a dot".into());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("file name must not contain path separators".into());
    }
    if name.contains("..") {
        return Err("file name must not contain '..'".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("file name must not contain control characters".into());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("file name must not end with a dot or space".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if WINDOWS_RESERVED_STEMS.contains(&stem.as_str()) {
        return Err("file name is reserved on Windows".into());
    }
    Ok(())
}

/// 최종 저장 경로 중복 회피: `name (2).ext`, `name (3).ext`, …
fn dedupe_final_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        // ".txt"처럼 확장자만 있는 이름은 validate_file_name이 이미 거부하므로
        // 여기서는 stem이 항상 비지 않는다.
        Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
        _ => (name.to_owned(), String::new()),
    };
    for index in 2.. {
        let candidate = dir.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("dedupe loop always returns")
}

/// 표시용 저장 위치(실제 경로 대신 사용자 친화적 형태만 응답으로 보낸다).
pub fn display_path(device_dir: &str, name: &str) -> String {
    format!("Downloads/leftcar/{device_dir}/{name}")
}

fn new_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    secure_channel::random_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 표준(base64, url-safe 아님) 디코딩 — 뷰어의 btoa 출력과 정확히 맞춘다.
fn decode_chunk(data_base64: &str) -> Result<Vec<u8>, String> {
    // 1 MiB 디코딩분의 표준 base64 길이는 1,398,104자를 넘지 않는다. 봉인
    // 프레임 한도(16 MiB) 안이지만, 조기 거부로 큰 문자열 디코딩 낭비를 피한다.
    if data_base64.len() > 1_400_000 {
        return Err("chunk is too large".into());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|_| "chunk is not valid base64".to_string())?;
    if decoded.len() > MAX_CHUNK_BYTES {
        return Err("chunk is too large".into());
    }
    Ok(decoded)
}

impl FileTransferState {
    /// 테스트 전용: incoming 루트를 임시 디렉터리로 고정한다.
    pub fn set_incoming_root_for_tests(&self, root: PathBuf) {
        *self.incoming_root.lock().unwrap() = Some(root);
    }

    fn device_dir(&self, device: &str) -> Result<PathBuf, String> {
        let root = match &*self.incoming_root.lock().unwrap() {
            Some(root) => root.clone(),
            None => {
                let Some(home) = dirs::home_dir() else {
                    return Err("cannot determine the home directory".into());
                };
                home.join("Downloads").join("leftcar")
            }
        };
        let dir = root.join(sanitize_device_name(device));
        std::fs::create_dir_all(&dir).map_err(|e| format!("create incoming dir: {e}"))?;
        Ok(dir)
    }

    /// 호스트 UI가 고른 파일을 공유 대기열에 올린다. 존재하지 않는 파일과
    /// 디렉터리, 상한 초과 파일은 거부 — 뷰어가 전 파일을 메모리에 받는 v1
    /// 특성상 fetch 방향도 같은 20 MiB 상한을 적용한다.
    pub fn add_share_file(&self, path: PathBuf) -> Result<ShareQueueEntry, String> {
        let metadata = std::fs::metadata(&path).map_err(|e| format!("stat: {e}"))?;
        if metadata.is_dir() {
            return Err("directories cannot be shared".into());
        }
        if metadata.len() > MAX_FILE_SIZE {
            return Err("file is too large".into());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| "path has no file name".to_owned())?;
        let entry = ShareQueueEntry {
            queue_id: new_token(),
            name,
            size: metadata.len(),
            path,
        };
        self.inner.lock().unwrap().queue.push(entry.clone());
        Ok(entry)
    }

    pub fn remove_share_file(&self, queue_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.queue.len();
        inner.queue.retain(|entry| entry.queue_id != queue_id);
        inner.queue.len() != before
    }

    /// 뷰어에 내보내는 대기열 뷰(로컬 경로 제외).
    pub fn queue_entries(&self) -> Vec<ShareQueueEntry> {
        self.inner.lock().unwrap().queue.clone()
    }

    /// 뷰어의 sendFileBegin — 스테이징 `.part` 파일을 만들고 토큰을 발급한다.
    /// 만료된 버려진 전송을 먼저 치우고, 동시 진행 수를 제한한다.
    pub fn begin_incoming(&self, device: &str, name: &str, size: u64) -> Result<String, String> {
        validate_file_name(name)?;
        if size > MAX_FILE_SIZE {
            return Err("file is too large".into());
        }
        let dir = self.device_dir(device)?;
        let token = new_token();
        let staging_path = dir.join(format!("{name}.{token}.part"));
        {
            let mut inner = self.inner.lock().unwrap();
            Self::sweep_with(&mut inner, STALE_TRANSFER);
            if inner.incoming.len() >= MAX_CONCURRENT_INCOMING {
                return Err("too many concurrent file transfers".into());
            }
            std::fs::File::create(&staging_path)
                .map_err(|e| format!("create staging file: {e}"))?;
            inner.incoming.insert(
                token.clone(),
                IncomingTransfer {
                    device: device.to_owned(),
                    name: name.to_owned(),
                    size,
                    written: 0,
                    staging_path,
                    last_activity: Instant::now(),
                },
            );
        }
        Ok(token)
    }

    /// `stale_after`보다 활동이 없던 전송을 버리고 스테이징 파일까지 지운다.
    /// 버린 뷰어가 End/Cancel 없이 사라진 경우의 안전망이다.
    fn sweep_with(inner: &mut Inner, stale_after: Duration) {
        Self::sweep_at(inner, stale_after, Instant::now());
    }

    fn sweep_at(inner: &mut Inner, stale_after: Duration, now: Instant) {
        let mut stale_parts: Vec<PathBuf> = Vec::new();
        inner.incoming.retain(|_, transfer| {
            if now.duration_since(transfer.last_activity) > stale_after {
                stale_parts.push(transfer.staging_path.clone());
                false
            } else {
                true
            }
        });
        inner
            .outgoing
            .retain(|_, transfer| now.duration_since(transfer.last_activity) <= stale_after);
        for part in stale_parts {
            let _ = std::fs::remove_file(part);
        }
    }

    /// 뷰어의 sendFileChunk — 소유 검사 + 순차 오프셋 강제 + 누적 상한.
    pub fn append_incoming(
        &self,
        device: &str,
        token: &str,
        data_base64: &str,
        offset: u64,
    ) -> Result<u64, String> {
        let data = decode_chunk(data_base64)?;
        let mut inner = self.inner.lock().unwrap();
        let transfer = inner
            .incoming
            .get_mut(token)
            .filter(|t| t.device == device)
            .ok_or("unknown file token")?;
        if offset != transfer.written {
            return Err("chunk offset is out of order".into());
        }
        if transfer.written.saturating_add(data.len() as u64) > transfer.size {
            return Err("chunk exceeds the declared file size".into());
        }
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&transfer.staging_path)
                .map_err(|e| format!("open staging file: {e}"))?;
            file.write_all(&data)
                .map_err(|e| format!("write staging file: {e}"))?;
        }
        transfer.written += data.len() as u64;
        // 활동 갱신 — 긴 전송이 만료 스윕에 잘리지 않게 한다(M2).
        transfer.last_activity = Instant::now();
        Ok(transfer.written)
    }

    /// 뷰어의 sendFileEnd — 크기가 맞아야 최종 이름으로 바꾼다. 모자라면
    /// 스테이징 파일을 지운다(불완전 파일 잔존 방지). 소유 검사를 remove
    /// 앞에 둔다 — 먼저 지우고 걸러내면 다른 기기의 잘못된 End가 실제
    /// 소유자의 전송 상태까지 파괴한다.
    pub fn finish_incoming(&self, device: &str, token: &str) -> Result<(String, u64), String> {
        let mut inner = self.inner.lock().unwrap();
        let transfer = remove_owned(&mut inner.incoming, token, device)?;
        if transfer.written != transfer.size {
            let _ = std::fs::remove_file(&transfer.staging_path);
            return Err("file transfer is incomplete".into());
        }
        let dir = transfer
            .staging_path
            .parent()
            .ok_or("staging path has no parent")?
            .to_path_buf();
        let final_path = dedupe_final_path(&dir, &transfer.name);
        std::fs::rename(&transfer.staging_path, &final_path)
            .map_err(|e| format!("finalize file: {e}"))?;
        Ok((transfer.name, transfer.written))
    }

    /// 뷰어의 sendFileCancel — 진행 중 업로드를 즉시 버리고 스테이징 파일을
    /// 지운다(만료 스윕을 기다리지 않는다).
    pub fn cancel_incoming(&self, device: &str, token: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_with(&mut inner, STALE_TRANSFER);
        let transfer = remove_owned(&mut inner.incoming, token, device)?;
        let _ = std::fs::remove_file(&transfer.staging_path);
        Ok(())
    }

    /// 뷰어의 fetchFileBegin — 대기열 항목이 아직 존재하는지 확인하고
    /// 내보내기 토큰을 발급한다. 파일이 사라졌거나 상한을 넘으면 오류.
    pub fn begin_outgoing(
        &self,
        device: &str,
        queue_id: &str,
    ) -> Result<(String, String, u64), String> {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_with(&mut inner, STALE_TRANSFER);
        let entry = inner
            .queue
            .iter()
            .find(|entry| entry.queue_id == queue_id)
            .ok_or("unknown queue entry")?;
        let size = std::fs::metadata(&entry.path)
            .map_err(|_| "shared file is missing".to_string())?
            .len();
        if size > MAX_FILE_SIZE {
            return Err("file is too large".into());
        }
        let name = entry.name.clone();
        let path = entry.path.clone();
        let token = new_token();
        inner.outgoing.insert(
            token.clone(),
            OutgoingTransfer {
                device: device.to_owned(),
                name: name.clone(),
                path,
                size,
                last_activity: Instant::now(),
            },
        );
        // 발급 시점 이름·크기를 돌려준다 — 전송 중 파일이 바뀌면 청크 범위
        // 검사가 등록된 크기로 통제한다.
        Ok((token, name, size))
    }

    /// 뷰어의 fetchFileChunk — 범위를 강제하고 표준 base64로 실어 보낸다.
    pub fn read_outgoing(
        &self,
        device: &str,
        token: &str,
        offset: u64,
        length: u64,
    ) -> Result<(String, u64), String> {
        if length == 0 || length as usize > MAX_CHUNK_BYTES {
            return Err("chunk length must be between 1 and 1 MiB".into());
        }
        let mut inner = self.inner.lock().unwrap();
        let transfer = inner
            .outgoing
            .get_mut(token)
            .filter(|t| t.device == device)
            .ok_or("unknown file token")?;
        if offset >= transfer.size {
            return Err("chunk offset is past the end of the file".into());
        }
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&transfer.path)
            .map_err(|_| "shared file is missing".to_string())?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("seek: {e}"))?;
        let mut buffer = vec![0u8; length.min(transfer.size - offset) as usize];
        file.read_exact(&mut buffer)
            .map_err(|e| format!("read: {e}"))?;
        // 활동 갱신 — 512MiB fetch도 만료 스윕에 잘리지 않게 한다(M2).
        transfer.last_activity = Instant::now();
        Ok((
            base64::engine::general_purpose::STANDARD.encode(&buffer),
            transfer.size,
        ))
    }

    /// 뷰어의 fetchFileEnd — 토큰을 정리하고 감사 대상 이름을 돌려준다.
    /// 소유 검사를 remove 앞에 둔다(finish_incoming과 같은 이유).
    pub fn finish_outgoing(&self, device: &str, token: &str) -> Result<String, String> {
        let mut inner = self.inner.lock().unwrap();
        let transfer = remove_owned(&mut inner.outgoing, token, device)?;
        Ok(transfer.name)
    }

    /// 뷰어의 fetchFileCancel — 내보내기 토큰을 즉시 버린다.
    pub fn cancel_outgoing(&self, device: &str, token: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_with(&mut inner, STALE_TRANSFER);
        remove_owned(&mut inner.outgoing, token, device)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "leftcar-ft-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn device_names_are_sanitized_to_safe_directory_names() {
        assert_eq!(sanitize_device_name("Galaxy S24 Ultra"), "Galaxy S24 Ultra");
        assert_eq!(sanitize_device_name("../etc/passwd"), "___etc_passwd");
        assert_eq!(sanitize_device_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_device_name(""), "device");
        assert_eq!(sanitize_device_name("   "), "device");
    }

    #[test]
    fn file_name_validation_rejects_traversal_and_hidden_names() {
        assert!(validate_file_name("report.pdf").is_ok());
        assert!(validate_file_name("파일 이름.txt").is_ok());
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("../x").is_err());
        assert!(validate_file_name("a/b").is_err());
        assert!(validate_file_name("a\\b").is_err());
        assert!(validate_file_name("..").is_err());
        assert!(validate_file_name("a..b").is_err());
        assert!(validate_file_name(".hidden").is_err());
        assert!(validate_file_name(&"x".repeat(256)).is_err());
        assert!(validate_file_name(&"x".repeat(255)).is_ok());
        // Windows 예약 장치 이름 — 확장자가 붙어도 스템 기준 거부.
        for reserved in ["CON", "nul.txt", "PRN.bin", "com1", "LPT9.part"] {
            assert!(validate_file_name(reserved).is_err(), "{reserved}");
        }
        assert!(validate_file_name("console.log").is_ok());
        // 후행 점·공백(Win32 정규화에서 사라져 이름 충돌/혼동을 만든다).
        assert!(validate_file_name("name.").is_err());
        assert!(validate_file_name("name ").is_err());
    }

    #[test]
    fn outgoing_rejects_files_over_the_cap() {
        let root = temp_dir("cap");
        let big = root.join("big.bin");
        // 희소 파일로 20 MiB+1을 만든다(실제 디스크 사용은 미미하다).
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_FILE_SIZE + 1)
            .unwrap();
        let state = FileTransferState::default();
        let error = state.add_share_file(big.clone()).unwrap_err();
        assert_eq!(error, "file is too large");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_transfers_are_swept_with_their_part_files() {
        let root = temp_dir("sweep");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());
        let token = state.begin_incoming("viewer-1", "a.txt", 4).unwrap();
        assert!(root
            .join("viewer-1")
            .join(format!("a.txt.{token}.part"))
            .exists());

        // begin 한 번 더 — 스윕은 begin에서 호출된다. stale_after를 0으로
        // 두면 방금 만든 전송이 즉시 만료다.
        {
            let mut inner = state.inner.lock().unwrap();
            FileTransferState::sweep_with(&mut inner, Duration::ZERO);
        }
        assert!(
            !root
                .join("viewer-1")
                .join(format!("a.txt.{token}.part"))
                .exists(),
            "stale .part must be deleted by the sweep"
        );
        assert_eq!(
            state.append_incoming("viewer-1", &token, "eA==", 0),
            Err("unknown file token".into()),
            "swept transfer must be gone from the map"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 만료는 마지막 활동 기준이다 — begin이 30분 전이라도 청크가 이어지는
    /// 전송은 스윕에 살아 남아야 한다(M2).
    #[test]
    fn fresh_chunk_activity_keeps_a_long_transfer_out_of_the_sweep() {
        // A freshly booted Windows runner cannot represent forty minutes ago.
        // A future marker proves that chunk I/O replaces the activity stamp;
        // an explicit sweep clock tests the full timeout without clock sleeps.
        let untouched_activity = Instant::now() + Duration::from_secs(40 * 60);

        let root = temp_dir("activity-in");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());
        let token = state.begin_incoming("viewer-1", "long.bin", 1).unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            inner.incoming.get_mut(&token).unwrap().last_activity = untouched_activity;
        }
        // 청크 트래픽이 활동을 갱신한다 — 스윕은 begin 시점이 아니라 이
        // 갱신 시점에서 만료를 계산해야 한다.
        state
            .append_incoming("viewer-1", &token, "eA==", 0)
            .unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            let activity = inner.incoming[&token].last_activity;
            assert!(activity < untouched_activity, "chunk must refresh activity");
            FileTransferState::sweep_at(&mut inner, STALE_TRANSFER, activity + STALE_TRANSFER);
        }
        assert!(
            state.finish_incoming("viewer-1", &token).is_ok(),
            "a transfer with fresh chunk activity must survive the sweep"
        );
        let _ = std::fs::remove_dir_all(&root);

        // 갱신 없이 30분이 지났다면 이번 스윕에서 치워진다.
        let root_idle = temp_dir("activity-idle");
        let state_idle = FileTransferState::default();
        state_idle.set_incoming_root_for_tests(root_idle.clone());
        let idle_token = state_idle
            .begin_incoming("viewer-1", "idle.bin", 4)
            .unwrap();
        {
            let mut inner = state_idle.inner.lock().unwrap();
            let activity = inner.incoming[&idle_token].last_activity;
            FileTransferState::sweep_at(
                &mut inner,
                STALE_TRANSFER,
                activity + STALE_TRANSFER + Duration::from_secs(1),
            );
        }
        assert_eq!(
            state_idle.append_incoming("viewer-1", &idle_token, "eA==", 0),
            Err("unknown file token".into()),
            "an idle transfer past the stale window must be swept"
        );
        let _ = std::fs::remove_dir_all(&root_idle);

        // fetch 방향도 같다 — 읽은 청크가 활동을 갱신한다.
        let root_out = temp_dir("activity-out");
        let source = root_out.join("video.mp4");
        std::fs::write(&source, b"0123456789abcdef").unwrap();
        let state_out = FileTransferState::default();
        let entry = state_out.add_share_file(source).unwrap();
        let (out_token, _, _) = state_out
            .begin_outgoing("viewer-1", &entry.queue_id)
            .unwrap();
        {
            let mut inner = state_out.inner.lock().unwrap();
            inner.outgoing.get_mut(&out_token).unwrap().last_activity = untouched_activity;
        }
        state_out
            .read_outgoing("viewer-1", &out_token, 0, 8)
            .unwrap();
        {
            let mut inner = state_out.inner.lock().unwrap();
            let activity = inner.outgoing[&out_token].last_activity;
            assert!(activity < untouched_activity, "read must refresh activity");
            FileTransferState::sweep_at(&mut inner, STALE_TRANSFER, activity + STALE_TRANSFER);
        }
        assert!(
            state_out.finish_outgoing("viewer-1", &out_token).is_ok(),
            "an outgoing transfer with fresh reads must survive the sweep"
        );
        let _ = std::fs::remove_dir_all(&root_out);
    }

    #[test]
    fn cancel_incoming_deletes_the_staging_file_immediately() {
        let root = temp_dir("cancel");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());
        let token = state.begin_incoming("viewer-1", "b.txt", 4).unwrap();
        state
            .append_incoming("viewer-1", &token, "eA==", 0)
            .unwrap();
        state.cancel_incoming("viewer-1", &token).unwrap();
        let leftover: Vec<_> = std::fs::read_dir(root.join("viewer-1")).unwrap().collect();
        assert!(leftover.is_empty(), "cancel must remove the staging file");
        assert_eq!(
            state.finish_incoming("viewer-1", &token),
            Err("unknown file token".into())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wrong_device_end_leaves_the_transfer_intact() {
        let root = temp_dir("owner");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());
        let token = state.begin_incoming("viewer-1", "c.txt", 1).unwrap();
        state
            .append_incoming("viewer-1", &token, "eA==", 0)
            .unwrap();
        // 다른 기기의 End/Cancel은 거부되고 실제 소유자의 전송은 살아 있다.
        assert_eq!(
            state.finish_incoming("viewer-2", &token),
            Err("unknown file token".into())
        );
        assert_eq!(
            state.cancel_incoming("viewer-2", &token),
            Err("unknown file token".into())
        );
        assert!(state.finish_incoming("viewer-1", &token).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn full_incoming_roundtrip_writes_the_final_file() {
        let root = temp_dir("roundtrip");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());

        // "Hello " + "world\n" = 12바이트.
        let token = state.begin_incoming("viewer-1", "hello.txt", 12).unwrap();
        assert_eq!(
            state
                .append_incoming("viewer-1", &token, "SGVsbG8g", 0)
                .unwrap(),
            6
        );
        assert_eq!(
            state
                .append_incoming("viewer-1", &token, "d29ybGQK", 6)
                .unwrap(),
            12
        );
        let (name, bytes) = state.finish_incoming("viewer-1", &token).unwrap();
        assert_eq!((name.as_str(), bytes), ("hello.txt", 12));
        assert_eq!(
            std::fs::read(root.join("viewer-1").join("hello.txt")).unwrap(),
            b"Hello world\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_arrivals_are_deduped_with_name_2_ext() {
        let root = temp_dir("dedupe");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());
        for _ in 0..2 {
            let token = state.begin_incoming("viewer-1", "a.txt", 1).unwrap();
            state
                .append_incoming("viewer-1", &token, "eA==", 0)
                .unwrap();
            state.finish_incoming("viewer-1", &token).unwrap();
        }
        let dir = root.join("viewer-1");
        assert!(dir.join("a.txt").exists());
        assert!(dir.join("a (2).txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn short_end_deletes_the_staging_file() {
        let root = temp_dir("short");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());

        let token = state.begin_incoming("viewer-1", "big.bin", 10).unwrap();
        state
            .append_incoming("viewer-1", &token, "YWJj", 0)
            .unwrap();
        assert_eq!(
            state.finish_incoming("viewer-1", &token),
            Err("file transfer is incomplete".into())
        );
        let leftover: Vec<_> = std::fs::read_dir(root.join("viewer-1")).unwrap().collect();
        assert!(leftover.is_empty(), "staging .part must be cleaned up");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn outgoing_queue_roundtrip_preserves_bytes_and_hides_local_paths() {
        let root = temp_dir("outgoing");
        let source = root.join("video.mp4");
        std::fs::write(&source, b"0123456789abcdef").unwrap();
        let state = FileTransferState::default();
        let entry = state.add_share_file(source.clone()).unwrap();
        assert_eq!(entry.name, "video.mp4");
        assert_eq!(entry.size, 16);
        // 직렬화 뷰에는 로컬 경로가 없어야 한다.
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("video.mp4/"), "{json}");
        assert!(!json.contains("path"), "{json}");

        let (token, name, size) = state.begin_outgoing("viewer-1", &entry.queue_id).unwrap();
        assert_eq!((name.as_str(), size), ("video.mp4", 16));
        let (first, total) = state.read_outgoing("viewer-1", &token, 0, 10).unwrap();
        assert_eq!(total, 16);
        let (second, _) = state.read_outgoing("viewer-1", &token, 10, 10).unwrap();
        let decode = |encoded: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap()
        };
        let mut bytes = decode(&first);
        bytes.extend(decode(&second));
        assert_eq!(bytes, b"0123456789abcdef");
        assert_eq!(
            state.finish_outgoing("viewer-1", &token).unwrap(),
            "video.mp4"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
