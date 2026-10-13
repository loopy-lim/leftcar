# Leftcar

![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)

Leftcar는 Mac 또는 Windows PC의 화면을 Android 휴대폰과 태블릿에서 빠르게 보고, 필요할 때 키보드와 포인터로 조작하는 다중 화면 뷰어다. 컴퓨터마다 Leftcar를 실행하고, Viewer 앱에서 원하는 화면을 별도 창으로 열 수 있다.

2026-09-13에 확인한 최신 공개 릴리스는 [v0.1.4](https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4)이며 `Leftcar-Viewer-0.1.4.apk`만 제공한다. 이 태그에 macOS·Windows 설치판이 함께 있다고 가정하지 않는다. 개발 후보와 공개 APK의 변경·호환성·서명 상태는 [지원 범위와 완료 기준](docs/completion-and-support.md), [버전·업데이트 안내](docs/versioning.md)를 확인한다.

## 한 문장 정의

> 사용자가 승인한 컴퓨터 디스플레이를 암호화된 연결로 전송하고, Android 기기에서 각각 독립된 창으로 보여 주는 도구

## 고정된 기본 범위

- 현재 개발 소스는 Host에서 화면 접근을 검토한 기기에 대해, 명시적으로 입력을 거부하지 않았다면 새 세션에 입력을 자동 허용한다. 세션에서 입력을 끄면 기기별 거부가 저장된다. OS 입력 권한은 별도로 필요하다. 포인터 전송률은 영상 FPS의 2배(60fps→120Hz, 90fps→180Hz)로 제한한다.
- 특정 기기 전용 기능 없이 일반 Android 앱의 다중 창 기능을 사용한다.
- 원격 source 하나를 Android Activity/task 인스턴스 하나에 연결해 여러 독립 창으로 보여 준다.
- 선택적인 Hub 창은 연결과 source 선택을 담당하고, 선택적인 Overview 창만 여러 타일을 한 화면에 모은다.
- 현재 캡처·화면 승인 대상은 디스플레이다. 앱 창 단독 캡처는 후속 목표이며, 가상 디스플레이 관리 코드는 별도 플랫폼 지원·수명 검사를 따른다.
- Rustra는 Rust와 TypeScript 사이의 명령, 상태, 오류 계약에 사용한다.
- 압축 영상과 고주파 입력 데이터는 Rustra나 JavaScript를 통과시키지 않고 별도 네이티브 데이터 경로로 전송한다.
- 아키텍처 목표는 TypeScript·Rust가 제품 로직을 맡고 Kotlin이 Android 플랫폼을 연결하는 것이다. 현재 KM의 Kotlin 네트워크·암호 정책은 이 경계를 벗어나며, 2026-10-13 검사에서 기존 위반 32건이 남아 있다.
- LAN 직접 연결을 기준선으로 하며, Tailscale 주소·별칭과 공개 WAN opt-in/UPnP 경로가 구현되어 있다. 자체 릴레이는 없고 외부망·Windows 실측 수용은 별도다.

## 문서

- [유사 프로젝트 비교와 개선·남은 격차](docs/research/2026-10-13-project-comparison.md)
- [문서 인덱스](docs/README.md)
- [지원 범위와 완료 기준](docs/completion-and-support.md)
- [마무리 후속 작업 검증](docs/2026-09-13-completion-followup-validation.md)
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

- 소스 설명 갱신: 2026-10-13. 공개 릴리스·과거 실측의 기준일은 각 기록을 따른다.
- 개발 후보에는 기기별 화면 승인과 저장된 입력 허용·거부, 연결·디코더 수명과 전송 복구 개선이 있다. 최신 후보의 실제 영상 연결과 10·30분 수용은 아직 완료되지 않았다. 공개 APK와 개발 소스의 검사 결과를 섞지 않는다. 현재 기준은 [지원 범위와 완료 기준](docs/completion-and-support.md), 과거 실측은 [구현 증거 문서](docs/EVIDENCE.md)에 구분한다.
- 구현: Rust workspace + Tauri 2 macOS/Windows Host + Expo/React Native Android Viewer + 네이티브 캡처/디코더 + CI
- 우선 대상 호스트: macOS
- 두 번째 대상 호스트: Windows (구현 있음; 이번 변경의 Windows 빌드·실기 수용 미검증)
- 선택적 후속 대상: Linux
- 우선 대상 뷰어: arm64 Android 휴대폰, 태블릿 및 대화면 기기

## 보안(요약)

개발 브랜치의 기기별 화면 승인과 정상/비정상 재시작 동작은 [화면·입력 승인 구현](docs/07-security-privacy.md#21-2026-09-13-개발-브랜치의-화면입력-승인)에 정리했다. 이 구현 설명을 기존 배포 파일이나 아직 수행하지 않은 실기기 검증에 소급 적용하지 않는다.

- 페어링 토큰과 승인 흐름으로 제어 평면 접근을 제한한다.
- 미디어 평면은 인증된 제어 peer와 연결된 경로 후보를 검증하고, 세션 키로 봉인한 왕복 확인을 거친 목적지로 전송한다. Tailscale·외부 주소를 사설 LAN 주소로 임의 대체하지 않는다.
- 입력 평면은 세션 키 AEAD로 인증하고 현재 기기·소스의 입력 승인이 유효한 경우에만 macOS CGEvent 또는 Windows SendInput으로 주입한다. 포인터 이동은 최신값 우선, 키와 버튼은 ACK/재시도 방식이다. Windows UIPI 때문에 일반 권한 Host는 관리자 권한 앱을 제어할 수 없다.
- 제어 채널은 QR로 핀된 호스트 Ed25519 정체 키 핸드셰이크 뒤 ChaCha20-Poly1305 AEAD로 봉인하고, 미디어 평면도 세션 키에서 HKDF로 도출한 방향별 키로 AEAD 봉인한다. 페어링 토큰은 macOS Keychain·Windows Credential Manager에 저장한다. Tailscale도 이 앱 인증을 생략하지 않는다. 공개 WAN 매핑은 기본 OFF이며 Host에서 켜야 한다. WAN 토글은 UPnP 매핑 설정이고 OS 방화벽 전체를 통제하는 기능은 아니다.

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

`bun run release:preflight -- --scope android-internal --json` 또는 `--scope host-macos-internal`로 구성·의존성·내부/공개 준비 상태를 먼저 확인할 수 있다. 실제 패키지의 버전·ABI·내장 코드·서명은 빌드 manifest에서도 검사한다. 남은 경고와 미검증 상태는 [버전·업데이트 안내](docs/versioning.md#빌드-전-검사와-소스-일치)를 따른다.

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
