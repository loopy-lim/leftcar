//! 호스트 클립보드 텍스트 접근 추상화(U5, docs/07 §20). 실제 앱에서는
//! tauri-plugin-clipboard-manager의 Rust API를 lib.rs setup에서 주입해 쓰고,
//! 테스트·플러그인 미주입 환경에서는 macOS pbcopy/pbpaste로 폴백한다
//! (문서에 허용된 경로 — 플러그인 Rust API가 이 버전에서 macOS 텍스트
//! 설정에 문제를 보이면 이 폴백이 곧 정식 구현이다).
//! 텍스트만 다룬다 — 이미지·파일은 범위 밖이다.

use tauri_plugin_clipboard_manager::ClipboardExt;

pub trait ClipboardBackend: Send + Sync {
    fn read_text(&self) -> Result<String, String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// tauri-plugin-clipboard-manager(데스크톱은 arboard) 백엔드.
pub struct TauriClipboard {
    app: tauri::AppHandle,
}

impl TauriClipboard {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl ClipboardBackend for TauriClipboard {
    fn read_text(&self) -> Result<String, String> {
        self.app.clipboard().read_text().map_err(|e| e.to_string())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.app.clipboard().write_text(text).map_err(|e| e.to_string())
    }
}

/// 플러그인이 주입되지 않은 환경(테스트, 등록 실패)의 폴백. macOS pbcopy/
/// pbpaste를 std::process::Command로 실행한다. 다른 플랫폼에서는 spawn이
/// 실패하고 그 오류가 그대로 명령 거부로 돌아간다(실제 앱은 항상 플러그인
/// 백엔드를 주입받는다).
pub struct SystemClipboard;

impl ClipboardBackend for SystemClipboard {
    fn read_text(&self) -> Result<String, String> {
        let output = std::process::Command::new("pbpaste")
            .output()
            .map_err(|e| format!("pbpaste: {e}"))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("pbcopy: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "pbcopy stdin unavailable".to_string())?
            .write_all(text.as_bytes())
            .map_err(|e| format!("pbcopy write: {e}"))?;
        let status = child.wait().map_err(|e| format!("pbcopy wait: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("pbcopy failed: {status}"))
        }
    }
}
