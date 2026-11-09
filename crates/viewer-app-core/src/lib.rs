//! 뷰어 앱 순수 코어 (계획 T-6, design §2.2).
//!
//! RN 뷰어의 제어 평면 클라이언트·세션 암호 절차·페어링·세션/재접속 상태머신을
//! Rust로 옮긴다. UI(Alert·router)와 네이티브 IO(SecureStore·TCP·난수·시계)는
//! 전부 주입 포트(트레잇)로만 만나며, 이 크레이트는 tokio 없이 std 스레드·채널·
//! 뮤텍스만 쓴다 — JSI 정적 라이브러리로 링크되는 것이 전제다.
//!
//! 모듈 경계:
//! - [`control`] — 개행 JSON 프레이밍·한도(16KiB/12MiB)·FIFO(32/16MiB)·명령별
//!   데드라인·토큰 주입·secure 핸드셰이크. 전송은 [`control::ControlTransport`]
//!   트레잇으로 주입한다.
//! - [`secure`] — crates/secure-channel 위의 뷰어 절차(ClientHello 생성·
//!   ServerHello 파싱·봉인 줄 프레이밍).
//! - [`pairing`] — QR 페이로드·엔드포인트·자격증명 스키마(v2/v3 키)·페어링 흐름.
//!   저장소는 [`pairing::CredentialStore`] 등으로 주입한다.
//! - [`session`] — 제어 세션 상태머신(선택 세대)·자동 재접속 게이트·페어링
//!   워크플로.

pub mod b64;
pub mod control;
pub mod pairing;
pub mod secure;
pub mod session;
