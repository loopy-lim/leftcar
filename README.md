# Leftcar

![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)

Leftcar는 Mac 또는 Windows PC의 화면을 Android 휴대폰과 태블릿에서 빠르게 보고, 필요할 때 키보드와 포인터로 조작하는 다중 화면 뷰어다. 컴퓨터마다 Leftcar를 실행하고, Viewer 앱에서 원하는 화면을 별도 창으로 열 수 있다.

설치 가능한 macOS Host와 Android Viewer는 [GitHub Releases](https://github.com/loopy-lim/leftcar/releases)에서 받을 수 있다. 최신 릴리스는 `v0.1.2`이며 macOS Apple Silicon DMG, Windows x64 unsigned NSIS 설치판, Android arm64 APK를 제공한다. 바이너리는 아직 서명·공증되지 않아 첫 실행 시 OS 경고 확인이 필요하다.

## 한 문장 정의

> 사용자가 선택한 컴퓨터 화면이나 앱 창을 신뢰하는 로컬 네트워크로 전송하고, Android 기기에서 각각 독립된 창으로 보여 주는 도구

## 고정된 기본 범위

- 개발 브랜치의 새 세션은 원격 입력이 꺼진 상태로 시작한다. Host에서 해당 기기의 화면 접근을 승인한 뒤, 세션별 입력을 별도로 허용해야 한다. OS 입력 권한도 필요하다. 포인터 전송률은 영상 FPS의 2배(60fps→120Hz, 90fps→180Hz)로 제한한다.
- 특정 기기 전용 기능 없이 일반 Android 앱의 다중 창 기능을 사용한다.
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
- [저지연 스트리밍 병목 조사](docs/11-low-latency-investigation.md)
- [Apple 화면 공유 비교 기준](docs/apple-screen-sharing-baseline.md)
- [Windows 원격 Host 구현과 검증](docs/windows-remote-host.md)
- [USB 물리 검증 절차](docs/usb-physical-validation.md)
- [구현 증거 문서](docs/EVIDENCE.md)
- [버전 문자열 현황](docs/versioning.md)
- [구현 계획](docs/plans/2026-08-17-leftcar-v1-implementation.md)

## 현재 상태

- 검증 기준일: 2026-09-01
- 상태: `v0.1.2` 릴리스 배포 중. QR 페어링 인증 + Android 미디어 수신 보안 + macOS/Windows 세션별 네이티브 원격 입력 + 적응형 해상도 복구 + 단일 인코더 세션 워치독 + UDP 안정화(FEC parity 4)까지 코드·빌드·CI 검증 반영. 입력 지연과 120/180Hz, Windows 실기기 스트리밍 계측은 대기 중이다. 상세는 [구현 증거 문서](docs/EVIDENCE.md) 참고
- 구현: Rust workspace + Tauri 2 macOS/Windows Host + Expo/React Native Android Viewer + 네이티브 캡처/디코더 + CI
- 우선 대상 호스트: macOS
- 두 번째 대상 호스트: Windows (코드 및 교차 컴파일 완료, 물리 E6/E7 대기)
- 선택적 후속 대상: Linux
- 우선 대상 뷰어: arm64 Android 휴대폰, 태블릿 및 대화면 기기

## 보안(요약)

개발 브랜치의 기기별 화면 승인과 정상/비정상 재시작 동작은 [화면·입력 승인 구현](docs/07-security-privacy.md#21-2026-09-13-개발-브랜치의-화면입력-승인)에 정리했다. 이 구현 설명을 기존 배포 파일이나 아직 수행하지 않은 실기기 검증에 소급 적용하지 않는다.

- 페어링 토큰과 승인 흐름으로 제어 평면 접근을 제한한다.
- 미디어 평면은 제어 peer와 같은 사설 LAN의 Viewer 후보만 허용하고, 예측 불가능한 난수의 UDP 왕복으로 실제 도달성을 증명한 주소에만 MTU 크기로 잘라 전송한다.
- 입력 평면은 같은 UDP 세션 난수로 인증하고 Host 사용자가 세션별로 허용한 경우에만 macOS CGEvent 또는 Windows SendInput으로 주입한다. 포인터 이동은 최신값 우선, 키와 버튼은 ACK/재시도 방식이다. Windows UIPI 때문에 일반 권한 Host는 관리자 권한 앱을 제어할 수 없다.
- 제어 채널은 QR로 핀된 호스트 Ed25519 정체 키 핸드셰이크 뒤 ChaCha20-Poly1305 AEAD로 봉인하고, 미디어 평면도 세션 키에서 HKDF로 도출한 방향별 키로 AEAD 봉인한다. 페어링 토큰은 macOS Keychain·Windows Credential Manager에 저장한다. 연결은 릴레이 없는 로컬 네트워크 직접 연결을 전제로 하며, 공개 인터넷 노출은 범위에 넣지 않는다.

## 빌드/실행

개발 도구의 기본은 **Bun 1.4.1**이다. Rust는 `rust-toolchain.toml`의 **1.95.0**을 사용한다. Expo/React Native와 Gradle 플러그인의 Node 호환 경로는 유지하며 CI 기준은 **Node 22.21.1**이다. 영상 처리의 Cargo·Swift·Gradle 경로는 그대로 사용한다.

```text
bun install --frozen-lockfile
bun run doctor js
bun run verify js
```

`bun run doctor`는 SDK·NDK·Java·Swift 등 전체 개발 환경을 확인하고, `bun run verify`는 JS·Rust·Host·Android JVM 검사를 함께 실행한다. 최초 브라우저 검사에 필요한 런타임은 `bun run setup:browser`로 준비한다. React 변경의 필수 검사는 `npx -y react-doctor@latest . --verbose`의 100점이며, 이후 타입 검사와 관련 테스트를 다시 실행한다. 이 검사는 실기기 스트리밍 성공을 대신하지 않는다.

개별 검사:

```text
bun install
bun run typecheck
bun run test
bun run test:contract
bun run test:architecture
cargo run -p control-contract --bin generate   # 생성물 갱신/검증
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
```

Android SDK/NDK와 `JAVA_HOME`을 준비한 뒤 `bun run build android-internal`, macOS에서는 `bun run build host-macos-internal`로 내부 시험 산출물을 만든다. 빌드는 저장소 밖에 소스·패키지·네이티브 파일 해시를 담은 manifest를 남긴다. `bun run release:manifest -- verify /absolute/path/build-manifest.json`으로 보관된 산출물의 일치를 확인한다. 내부 APK는 기존 디버그 서명 파일을 사용하며, 별도 위치라면 `LEFTCAR_INTERNAL_DEBUG_KEYSTORE`에 그 절대 경로를 지정한다. 내부 빌드는 공개 서명·공증·배포를 수행하지 않는다.

공개 Android 빌드는 `bun run build android-release`이며 릴리스 키 설정이 별도로 필요하다. 자세한 환경과 검증 범위는 [개발 안내](CONTRIBUTING.md)를 참고한다.

macOS Host 개발은 아래 명령을 사용한다. Apple Development 서명 요구사항이
설치본과 같은지 확인한 뒤 `/Applications/Leftcar Host.app`을 제자리에서 교체하고
하나만 실행하므로, 최초 승인한 화면 기록 권한을 이후 빌드에서도 재사용한다.

```text
bun run dev:host:macos
```

서명 요구사항이 달라지면 화면 기록 권한이 초기화될 수 있으므로 설치를 중단한다.
일반 `tauri dev` 실행 파일과 `/Applications` 설치본을 동시에 실행하지 않는다.

Windows x64 Host는 Windows 머신에서 다음 명령으로 current-user NSIS 설치 파일을 만든다.

```text
bun run --cwd apps/host-desktop tauri build
```

### 로컬 검증 명령

```text
cargo test --workspace          # E1: 전 crate 단위/property 테스트
cargo clippy --workspace --tests -- -D warnings
bun run test && bun run test:contract # TS 단위 + 계약 테스트
bun run test:architecture       # ADR-0002 의존성/TS/Kotlin 규칙
bun run rustra:generate         # 생성 코드 재생성 (diff 없어야 함)
```

추가 문서:

- [DESIGN.md](./DESIGN.md)
