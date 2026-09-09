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

/// 뷰어 → 호스트 파일 상한(20 MiB).
pub const MAX_FILE_SIZE: u64 = 20 * 1024 * 1024;
/// 청크 1개(디코딩 후) 상한. fetch와 send가 같은 상한을 쓴다.
pub const MAX_CHUNK_BYTES: usize = 1024 * 1024;
/// 파일 토큰 바이트 수(16B → hex 32자).
const TOKEN_BYTES: usize = 16;

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
}

struct OutgoingTransfer {
    device: String,
    name: String,
    path: PathBuf,
    size: u64,
}

#[derive(Default)]
struct Inner {
    incoming: HashMap<String, IncomingTransfer>,
    outgoing: HashMap<String, OutgoingTransfer>,
    queue: Vec<ShareQueueEntry>,
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

/// 업로드 파일명 검증. 경로 구분자·`..`·선행 점(숨김 파일)을 거부하고
/// 길이는 1..=255바이트로 제한한다. Ok(())면 단일 파일명으로 안전하다.
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

    /// 호스트 UI가 고른 파일을 공유 대기열에 올린다. 존재하지 않는 파일은 거부.
    pub fn add_share_file(&self, path: PathBuf) -> Result<ShareQueueEntry, String> {
        let metadata = std::fs::metadata(&path).map_err(|e| format!("stat: {e}"))?;
        if metadata.is_dir() {
            return Err("directories cannot be shared".into());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| "path has no file name".to_string())?;
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
    pub fn begin_incoming(&self, device: &str, name: &str, size: u64) -> Result<String, String> {
        validate_file_name(name)?;
        if size > MAX_FILE_SIZE {
            return Err("file is too large".into());
        }
        let dir = self.device_dir(device)?;
        let token = new_token();
        let staging_path = dir.join(format!("{name}.{token}.part"));
        std::fs::File::create(&staging_path).map_err(|e| format!("create staging file: {e}"))?;
        self.inner.lock().unwrap().incoming.insert(
            token.clone(),
            IncomingTransfer {
                device: device.to_owned(),
                name: name.to_owned(),
                size,
                written: 0,
                staging_path,
            },
        );
        Ok(token)
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
        Ok(transfer.written)
    }

    /// 뷰어의 sendFileEnd — 크기가 맞아야 최종 이름으로 바꾼다. 모자라면
    /// 스테이징 파일을 지운다(불완전 파일 잔존 방지).
    pub fn finish_incoming(&self, device: &str, token: &str) -> Result<(String, u64), String> {
        let mut inner = self.inner.lock().unwrap();
        let transfer = inner
            .incoming
            .remove(token)
            .filter(|t| t.device == device)
            .ok_or("unknown file token")?;
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

    /// 뷰어의 fetchFileBegin — 대기열 항목이 아직 존재하는지 확인하고
    /// 내보내기 토큰을 발급한다. 파일이 사라졌다면 오류.
    pub fn begin_outgoing(
        &self,
        device: &str,
        queue_id: &str,
    ) -> Result<(String, String, u64), String> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .queue
            .iter()
            .find(|entry| entry.queue_id == queue_id)
            .ok_or("unknown queue entry")?;
        let size = std::fs::metadata(&entry.path)
            .map_err(|_| "shared file is missing".to_string())?
            .len();
        let name = entry.name.clone();
        let path = entry.path.clone();
        let token = new_token();
        inner.outgoing.insert(
            token.clone(),
            OutgoingTransfer {
                device: device.to_owned(),
                name,
                path,
                size,
            },
        );
        // 발급 시점 이름·크기를 돌려준다 — 전송 중 파일이 바뀌면 청크 범위
        // 검사가 등록된 크기로 통제한다.
        let registered = inner.outgoing.get(&token).unwrap();
        Ok((token, registered.name.clone(), registered.size))
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
        let inner = self.inner.lock().unwrap();
        let transfer = inner
            .outgoing
            .get(token)
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
        Ok((
            base64::engine::general_purpose::STANDARD.encode(&buffer),
            transfer.size,
        ))
    }

    /// 뷰어의 fetchFileEnd — 토큰을 정리하고 감사 대상 이름을 돌려준다.
    pub fn finish_outgoing(&self, device: &str, token: &str) -> Result<String, String> {
        let transfer = self
            .inner
            .lock()
            .unwrap()
            .outgoing
            .remove(token)
            .filter(|t| t.device == device)
            .ok_or("unknown file token")?;
        Ok(transfer.name)
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
    }

    #[test]
    fn full_incoming_roundtrip_writes_the_final_file() {
        let root = temp_dir("roundtrip");
        let state = FileTransferState::default();
        state.set_incoming_root_for_tests(root.clone());

        // "Hello " + "world\n" = 12바이트.
        let token = state.begin_incoming("viewer-1", "hello.txt", 12).unwrap();
        assert_eq!(state.append_incoming("viewer-1", &token, "SGVsbG8g", 0).unwrap(), 6);
        assert_eq!(state.append_incoming("viewer-1", &token, "d29ybGQK", 6).unwrap(), 12);
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
        state.append_incoming("viewer-1", &token, "YWJj", 0).unwrap();
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
            base64::engine::general_purpose::STANDARD.decode(encoded).unwrap()
        };
        let mut bytes = decode(&first);
        bytes.extend(decode(&second));
        assert_eq!(bytes, b"0123456789abcdef");
        assert_eq!(state.finish_outgoing("viewer-1", &token).unwrap(), "video.mp4");
        let _ = std::fs::remove_dir_all(&root);
    }
}
