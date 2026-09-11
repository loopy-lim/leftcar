//! 세션 종료 후 화면 잠금(설정 `lock_on_disconnect`). 플랫폼별 1회성
//! 명령으로 구현한다 — 상주 프로세스나 권한 상승이 필요 없는 경로만 쓴다.
//! 실패는 로그만 남긴다(잠금은 편의 기능이지 세션 안전성과 무관하다).

/// 현재 로그인 세션을 즉시 잠근다.
pub fn lock_workstation() {
    let status = lock_command_status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("leftcar: screen lock command exited {status}"),
        Err(error) => eprintln!("leftcar: screen lock command failed to start: {error}"),
    }
}

#[cfg(target_os = "macos")]
fn lock_command_status() -> std::io::Result<std::process::ExitStatus> {
    use std::process::Command;
    // CGSession -suspend가 현재 로그인 세션을 즉시 잠근다. 경로가 없어진
    // 최신 macOS에서는 디스플레이 절전으로 대체한다 — 잠금 설정이 켜져
    // 있으면 깨울 때 잠긴다.
    let cgsession = std::path::Path::new(
        "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
    );
    if cgsession.exists() {
        Command::new(cgsession).arg("-suspend").status()
    } else {
        Command::new("pmset").arg("displaysleepnow").status()
    }
}

#[cfg(target_os = "windows")]
fn lock_command_status() -> std::io::Result<std::process::ExitStatus> {
    use std::process::Command;
    Command::new("rundll32")
        .args(["user32.dll,LockWorkStation"])
        .status()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn lock_command_status() -> std::io::Result<std::process::ExitStatus> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "screen lock is not implemented on this platform",
    ))
}
