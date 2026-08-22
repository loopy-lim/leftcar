# Leftcar

![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)

Leftcar는 macOS 또는 Windows 화면을 Galaxy XR과 Android 기기에서 빠르게 보고, 필요할 때 키보드와 포인터로 제어하는 다중 화면 뷰어다. macOS Host는 ScreenCaptureKit/VideoToolbox를, Windows Host는 Windows Graphics Capture/Media Foundation 하드웨어 H.264를 사용하고, Android Viewer는 네이티브 MediaCodec 파이프라인으로 영상을 표시한다.

설치 가능한 macOS Host와 Android Viewer는 [GitHub Releases](https://github.com/loopy-lim/leftcar/releases)에서 받을 수 있다. Windows x64 Host는 unsigned NSIS 패키지 CI까지 구현했으며, 실제 Windows/GPU와 Android 기기의 E6 검증 및 서명 릴리스는 아직 대기 중이다.

## 한 문장 정의

> 사용자가 선택한 데스크톱 화면이나 앱 창 여러 개를 로컬 네트워크로 전송하고, Galaxy XR Home Space에서 각각 독립된 Android 창으로 낮은 지연으로 보여 주는 도구

## 고정된 기본 범위

- 원격 입력은 세션마다 Host에서 명시적으로 켜야 하며 기본값은 꺼짐이다. 포인터 전송률은 영상 FPS의 2배(60fps→120Hz, 90fps→180Hz)로 제한한다.
- Android XR 전용 기능 없이 Home Space에서 실행되는 일반 Android 앱으로 시작한다.
- 원격 source 하나를 Android Activity/task 인스턴스 하나에 연결해 여러 독립 창으로 보여 준다.
- 선택적인 Hub 창은 연결과 source 선택을 담당하고, 선택적인 Overview 창만 여러 타일을 한 화면에 모은다.
- 실제 OS 가상 모니터 드라이버보다 앱 창 또는 물리 디스플레이 캡처를 먼저 지원한다.
- Rustra는 Rust와 TypeScript 사이의 명령, 상태, 오류 계약에 사용한다.
- 압축 영상과 고주파 입력 데이터는 Rustra나 JavaScript를 통과시키지 않고 별도 네이티브 데이터 경로로 전송한다.
- 제품 로직은 TypeScript와 Rust로 작성한다. Kotlin은 Activity, Intent, Surface와 Android 플랫폼 입력 이벤트를 네이티브 코어에 연결하는 shim으로 제한한다.
- 로컬 네트워크 직접 연결을 첫 배포 범위로 한다.

## 문서

- [문서 인덱스](docs/README.md)
- [제품 요구사항](docs/01-product-requirements.md)
- [기술 타당성 조사](docs/02-feasibility-research.md)
- [시스템 아키텍처](docs/03-system-architecture.md)
- [Rustra 제어 계약](docs/04-rustra-control-contracts.md)
- [TDD와 품질 전략](docs/05-tdd-quality-strategy.md)
- [성능 측정 계획](docs/06-benchmark-device-validation.md)
- [보안과 개인정보 보호](docs/07-security-privacy.md)
- [24주 구현 계획](docs/08-implementation-roadmap.md)
- [위험과 미결정 사항](docs/09-risk-register.md)
- [공식 근거 자료](docs/10-references.md)
- [Apple 화면 공유 비교 기준](docs/apple-screen-sharing-baseline.md)
- [Windows 원격 Host 구현과 검증](docs/windows-remote-host.md)
- [구현 증거 문서](docs/EVIDENCE.md)
- [구현 계획](docs/plans/2026-08-17-leftcar-v1-implementation.md)

## 현재 상태

- 검증 기준일: 2026-08-22
- 상태: `v0.1.0` 구현 + QR 페어링 인증 + Android 미디어 수신 보안 + macOS/Windows 세션별 네이티브 원격 입력의 코드·빌드 검증 반영. 입력 지연과 120/180Hz, Windows 실기기 스트리밍 계측은 대기 중이다. 상세는 [구현 증거 문서](docs/EVIDENCE.md) 참고
- 구현: Rust workspace + Tauri 2 macOS/Windows Host + Expo/React Native Android Viewer + 네이티브 캡처/디코더 + CI
- 우선 대상 호스트: macOS
- 두 번째 대상 호스트: Windows (코드 및 교차 컴파일 완료, 물리 E6/E7 대기)
- 선택적 후속 대상: Linux
- 우선 대상 뷰어: Galaxy XR 및 arm64 Android 기기

## 보안(요약)

- 페어링 토큰과 승인 흐름으로 제어 평면 접근을 제한한다.
- 미디어 평면은 제어 peer와 같은 사설 LAN의 Viewer 후보만 허용하고, 예측 불가능한 난수의 UDP 왕복으로 실제 도달성을 증명한 주소에만 MTU 크기로 잘라 전송한다.
- 입력 평면은 같은 UDP 세션 난수로 인증하고 Host 사용자가 세션별로 허용한 경우에만 macOS CGEvent 또는 Windows SendInput으로 주입한다. 포인터 이동은 최신값 우선, 키와 버튼은 ACK/재시도 방식이다. Windows UIPI 때문에 일반 권한 Host는 관리자 권한 앱을 제어할 수 없다.
- **제어는 인증된 평문 TCP, 미디어는 평문 UDP**이므로 로컬 네트워크 또는 Tailscale 내에서 사용한다. 공개 인터넷 사용에는 TLS/PAKE 기반 상호 인증과 미디어 암호화가 필요하다.

## 빌드/실행

```text
pnpm install
pnpm typecheck
pnpm test
pnpm test:contract
pnpm test:architecture
cargo run -p control-contract --bin generate   # 생성물 갱신/검증
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
```

운영 빌드/릴리스는 로컬 환경 변수(Tauri CLI, Android SDK/NDK PATH, Rust toolchain)를 맞춘 뒤 해당 타겟 빌드 파이프라인을 수행한다.

Windows x64 Host는 Windows 머신에서 다음 명령으로 current-user NSIS 설치 파일을 만든다.

```text
pnpm --filter @leftcar/host-desktop tauri build
```

### 로컬 검증 명령

```text
cargo test --workspace          # E1: 전 crate 단위/property 테스트
cargo clippy --workspace --tests -- -D warnings
pnpm test && pnpm test:contract # TS 단위 + 계약 테스트
pnpm test:architecture          # ADR-0002 의존성/TS/Kotlin 규칙
pnpm rustra:generate            # 생성 코드 재생성 (diff 없어야 함)
```

추가 문서:

- [DESIGN.md](./DESIGN.md)
