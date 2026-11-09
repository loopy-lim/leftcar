//! 호스트 UI 순수 로직의 Rust 포팅(계획 T-2). React 앱의 순수 모듈과 그
//! vitest 스펙을 같은 규칙으로 옮긴다:
//! - `termination`  ← src/streamTermination.ts (+streamTermination.test.ts)
//! - `toggle_gate`  ← src/toggleGate.ts (+Privacy.test.ts의 toggle gate 케이스)
//! - `paired_device_state` ← src/paired-device-state.ts (모듈 전역 저장소를
//!   `&mut self` 구조체로)
//! - `session_row`  ← src/sessionTypes.ts(SessionRow JSON 계약)
//! - `encoder_diagnostics` ← src/encoderDiagnostics.ts (+encoderDiagnostics.test.ts)
//! - `host_state`   ← src/hostState.ts (+hostState.test.ts)
//! - `error_view`   ← src/hooks/useHostStatus.ts의 hostErrorView 분류
//!
//! 언어 타입은 lib 크레이트의 [`leftcar_host_desktop::settings::HostLanguage`]
//! 를 그대로 쓴다 — set_language 명령이 영속하는 값과 같은 타입이다.

// 순수 모듈들은 T-3 UI가 소비한다 — 그때까지 bin 크레이트의 미사용 경고만
// 끈다(테스트는 이미 전부를 소비한다).
#![allow(dead_code)]

pub mod encoder_diagnostics;
pub mod error_view;
pub mod host_state;
pub mod paired_device_state;
pub mod session_row;
pub mod termination;
pub mod toggle_gate;
