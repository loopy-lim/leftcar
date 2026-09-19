//! 기기별 스트림 창 크기 영속 저장(2026-09-20 XR 창 크기 유지).
//!
//! 뷰어가 XR 시스템 핸들로 창 크기를 조정하면 그 절대 픽셀 크기를
//! `setWindowSize`로 보고하고, 호스트는 기기별로 기억해 다음 `getCatalog`에
//! 되돌려준다 — 새 창이 시스템 기본 크기로 리셋되지 않게 하기 위해서다.
//! 키는 그랜트 저장소와 같은 owner id다. 창 크기는 보안 값이 아니므로
//! 그랜트와 달리 저장 실패가 기능을 막지 않는다(메모리 값도 갱신하지 않는
//! 낙상한 커밋만 유지).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use control_contract::host::WindowSizeView;

/// 프로토콜 오염·말도 안 되는 값은 저장하지 않는다. XR 패널 실측
/// (1942×1092)과 시스템 상한(max=3840×2700)을 감안한 상식 범위다.
pub fn validated(width_px: u32, height_px: u32) -> Option<WindowSizeView> {
    const MIN_PX: u32 = 200;
    const MAX_PX: u32 = 8192;
    if (MIN_PX..=MAX_PX).contains(&width_px) && (MIN_PX..=MAX_PX).contains(&height_px) {
        Some(WindowSizeView {
            width_px,
            height_px,
        })
    } else {
        None
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    version: u32,
    devices: BTreeMap<String, WindowSizeView>,
}

/// 기기 owner id → 마지막 보고 창 크기. `path`가 없으면 메모리에만 산다.
pub struct WindowMetricsStore {
    path: Option<PathBuf>,
    journal: Mutex<Journal>,
}

impl WindowMetricsStore {
    /// 손상되었거나 알 수 없는 버전의 파일은 settings와 같은 정책으로
    /// 기본값(빈 지도)으로 되돌아간다. 읽기 실패가 기능을 죽이지 않는다.
    pub fn open(path: Option<PathBuf>) -> Self {
        let journal = path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok())
            .filter(|journal| journal.version == 1)
            .unwrap_or(Journal {
                version: 1,
                devices: BTreeMap::new(),
            });
        Self {
            path,
            journal: Mutex::new(journal),
        }
    }

    pub fn get(&self, owner: &str) -> Option<WindowSizeView> {
        self.journal.lock().unwrap().devices.get(owner).copied()
    }

    /// 같은 값 재보고(적용 에코)는 디스크 쓰기 없이 받아 넘긴다.
    pub fn set(&self, owner: &str, size: WindowSizeView) -> Result<(), String> {
        let mut journal = self.journal.lock().unwrap();
        if journal.devices.get(owner) == Some(&size) {
            return Ok(());
        }
        let mut devices = journal.devices.clone();
        devices.insert(owner.to_owned(), size);
        if let Some(path) = &self.path {
            let body = serde_json::to_vec(&Journal {
                version: 1,
                devices: devices.clone(),
            })
            .map_err(|e| e.to_string())?;
            crate::source_grants::durable_replace(path, &body)?;
        }
        journal.devices = devices;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("leftcar-window-metrics-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reported_size_roundtrips_within_process() {
        let store = WindowMetricsStore::open(None);
        store.set("owner-a", WindowSizeView { width_px: 1942, height_px: 1092 }).unwrap();
        assert_eq!(
            store.get("owner-a"),
            Some(WindowSizeView { width_px: 1942, height_px: 1092 })
        );
        assert_eq!(store.get("owner-b"), None);
    }

    #[test]
    fn size_persists_across_reopen() {
        let dir = tempdir();
        let path = dir.join("window_metrics.json");
        let store = WindowMetricsStore::open(Some(path.clone()));
        store.set("owner-a", WindowSizeView { width_px: 1600, height_px: 900 }).unwrap();
        drop(store);
        let reopened = WindowMetricsStore::open(Some(path));
        assert_eq!(
            reopened.get("owner-a"),
            Some(WindowSizeView { width_px: 1600, height_px: 900 })
        );
    }

    #[test]
    fn identical_report_skips_disk_write() {
        let dir = tempdir();
        let path = dir.join("window_metrics.json");
        let store = WindowMetricsStore::open(Some(path.clone()));
        store.set("owner-a", WindowSizeView { width_px: 1600, height_px: 900 }).unwrap();
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        store.set("owner-a", WindowSizeView { width_px: 1600, height_px: 900 }).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
    }

    #[test]
    fn corrupt_file_falls_back_to_empty_journal() {
        let dir = tempdir();
        let path = dir.join("window_metrics.json");
        std::fs::write(&path, b"{not json").unwrap();
        let store = WindowMetricsStore::open(Some(path));
        assert_eq!(store.get("owner-a"), None);
    }
}
