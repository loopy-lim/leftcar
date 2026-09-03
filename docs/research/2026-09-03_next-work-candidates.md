---
date: 2026-09-03T12:45:33+09:00
researcher: loopy-lim
git_commit: d4afeb8d021901eb41bf515f0f4db14dde807d52
branch: main
repository: leftcar
topic: "다방면 다음 작업 후보 조사 (진행 상태·미완료·검증 게이트·리서치 기반)"
tags: [research, codebase, roadmap, evidence, virtual-display, transport, usb-aoap, cursor]
status: complete
last_updated: 2026-09-03
last_updated_by: loopy-lim
---

# 리서치: 다음 작업 후보 다방면 조사

**날짜**: 2026-09-03 12:45:33 +0900
**연구자**: loopy-lim
**Git Commit**: d4afeb8d021901eb41bf515f0f4db14dde807d52
**Branch**: main
**Repository**: leftcar
**방법**: 병렬 서브에이전트 3건 (코드 미완료 표식 / docs 검증·로드맵 상태 / 테스트·빌드 인프라) + EVIDENCE·검증 문서 직접 확인

## 연구 질문

현재 leftcar에서 더 작업하고 진행할 만한 것들이 무엇인지 다방면으로 조사한다.

## 요약

이 저장소는 TODO/FIXME 주석이 0건에 가깝고, "미완료"가 **E등급 증거 언어(E0–E7), 옵트인 게이트, ADR/Risk 문서**로 표현되는 특이한 구조다. 세 갈래 조사를 종합하면 다음 작업 후보는 5개 군으로 수렴한다:

1. **미커밋 리서치 커밋** — `docs/research/2026-09-03_opendisplay-comparison.md`가 untracked로 남아 있다. 오늘 작성한 문서라 유실 위험이 있고, 이 문서가 아래 3·4번 작업의 근거다.
2. **T11 USB 물리 검증 잔여 수행** — 절차 문서는 완비됐고 검증 1·2·3·5는 Pass. 남은 것은 **검증 4(60분 soak, 미수행)**와 **검증 6 잔여(앱 UI 경유 가상 디스플레이 생성→스트리밍→태블릿 렌더링)**. 이 게이트가 끝나야 AOAP 전송과 가상 디스플레이가 E3 실험 상태에서 탈출한다 (`docs/usb-physical-validation.md:9`).
3. **가상 디스플레이 다음 단계** — 코드·게이트는 완성됐고 남은 것은 (a) 앱 UI 경유 end-to-end 실기 검증, (b) CGVirtualDisplay private API 하루 스파크(오늘 리서치가 R-015의 evidence 수집 단계로 정합)와 승격 여부 별도 ADR.
4. **커서 분리** — `showsCursor = true`가 남아 있는 "가장 명확한 격차". OpenDisplay 참고 구현이 확보됐고, 미해결 질문은 LCS1/LCI1 채널 설계뿐.
5. **transport 결정(G2)** — `transport-quic` 크레이트가 전체가 placeholder(`NOT IMPLEMENTED YET by design`)이며 ADR-0004(제안 상태)가 bake-off 전 확정 금지. bake-off 실행 또는 의도적 보류 선언이 필요.

그 외: 설계 크레이트 8종이 프로덕션 바이너리에 미연결(host-core는 fake-only), v0.2-hardening 플랜이 체크박스 전부 미체크 상태, 4K/1440p 고모션/입력 주입 등 실기 계측 묶음이 대기 중.

## 상세 분석

### A. 즉시 실행 가능 (기기 불필요, 코드·문서만으로)

#### A1. 미커밋 리서치 커밋 (즉시)
- `docs/research/2026-09-03_opendisplay-comparison.md` — git status 유일 untracked. GPL-3.0 경계, CGVirtualDisplay 최초 검토 기록, 미해결 질문 5건을 담고 있다.

#### A2. transport-quic 구현 또는 bake-off 준비
- `crates/transport-quic/src/lib.rs:3` — `//! NOT IMPLEMENTED YET by design: ADR-0004 defers the WebRTC-vs-QUIC decision...` 크레이트 전체 placeholder.
- `SelectedTransport::Undecided`가 유일 기본값이고 `ProductBuildInfo::new(Undecided)`가 제품 빌드를 거부(H14 Red, lib.rs:48에 red 테스트만 존재).
- G2/H11–H14 bake-off는 `docs/EVIDENCE.md:60-72` 표에서 Galaxy XR 실기기를 요구 — 기기 확보 전에는 **QUIC reliable stream + DATAGRAM 구현 자체**(L5 loopback으로 검증 가능한 부분)가 선행 작업이 될 수 있다.
- 관련 미결정 질문 Q-003(WebRTC vs QUIC), Q-007(HEVC 포함 여부) — `docs/09-risk-register.md:119-136`.

#### A3. 커서 분리 (참고 구현 확보됨)
- `native/macos-capture-shim/Sources/Capture/CaptureSession+Backend.swift:102` — `config.showsCursor = true` (비디오에 커서 구움).
- 2026-08-25 리서치가 "업계 표준, Leftcar 미달"로 결론, 오늘 리서치가 OpenDisplay 참고 구현(캡처에서 숨김 + 120Hz JSON + UDP 9001 사이드 채널 로컬 렌더)까지 확보.
- 설계 과제: LCS1/LCI1 채널에 커서 좌표 역방향 스트림을 넣는 방식 (UDP 사이드 채널 vs 제어 채널 확장).

#### A4. CGVirtualDisplay 하루 스파크 → 별도 ADR
- 저장소에 CGVirtualDisplay 검토 기록은 오늘 리서치가 최초(이전 ADR-0003/0005는 전부 DriverKit 전제).
- 스파크 범위(오늘 리서치 미해결 질문 1–2): 현재 macOS에서 private API 동작 확인(60Hz 상한, `.forSession` 미러 해제, HiDPI 재적용 루프), Leftcar 420v 캡처→VT→전송 경로 정합 실측.
- 주의: R-015(`docs/09-risk-register.md:30`)의 v1 논골 유지 — 스파크는 evidence 수집이지 정식 기능 승격이 아니다.

#### A5. 설계 크레이트의 프로덕션 연결 (구조적 부채)
- `apps/host-desktop/src-tauri/Cargo.toml:30-34`, `native/android-viewer/Cargo.toml:12-16`에 의존성이 없는 크레이트 8종: host-core, media-model, transport-api, transport-quic, macos-capture, macos-encode, network-protocol, diagnostics.
- `crates/host-core/src/lib.rs:1,15` — host-core는 fake 경로(FakeCapture/FakeEncoder)로만 검증됨. 실제 SCK/VT 어댑터는 facade에 존재하나 오케스트레이션이 프로덕션 바이너리에 미연결.
- 단, 현재 프로덕션 경로(native shim)가 실기 검증을 통과한 상태라 이것은 "리팩터링 과제"이지 "버그"가 아니다 — 착 시 시 ADR 수준의 근거 정리 권장.

#### A6. ADR 상태 정리
- ADR-0001~0004가 전부 "제안" 상태(각 파일 3행), ADR-0005만 "승인 — 옵트인 실험". 실제 구현이 진행된 0001/0002는 확정으로 승격 검토 대상.

### B. 실기기 게이트 (기기+시간 필요, 절차는 문서화 완료)

| 항목 | 상태 | 근거 |
|---|---|---|
| T11 검증 4: 60분 USB soak | **미수행** | `docs/usb-physical-validation.md:143,155` |
| T11 검증 6 잔여: 앱 UI 경유 VD 생성→스트리밍→태블릿 렌더 | 미수행 (CLI 직접은 Pass) | `docs/usb-physical-validation.md:145,153-154` |
| 입력 주입 실기 검증 | 실기 표본이 `inputEnabled=false`였음 | `docs/EVIDENCE.md:158` |
| 4K 고모션 병목 (macOS capture/encode path) | 병목 위치 판정까지만 | `docs/EVIDENCE.md:190` |
| 1440p 고모션 60 unique fps 판정 | frameGaps=72, outputDrops=320 잔존 | `docs/EVIDENCE.md:216` |
| glass-to-glass p50/p95 (H21) | 240fps 카메라 200 sample 필요 | `docs/EVIDENCE.md:68` |
| 90fps 기본값 재노출 | 60Hz 소스 악화 확인으로 코드에서 숨김 — 90Hz 소스 재검증 필요 | `docs/EVIDENCE.md:157` |
| macOS notarization + v0.2 릴리스 서명 | 미수행 | `docs/EVIDENCE.md:159`, plans v0.2-hardening 226-228행 |
| Windows 물리 E6/E7, NSIS CI artifact | 소스·교차컴파일까지만 | `docs/EVIDENCE.md:79` |
| Galaxy XR G1 잔여 (4창/비초점/4디코더) | 기기 미보유(R-017) | `docs/EVIDENCE.md:64-66` |

B군의 특징: **skip된 테스트가 0건**인 대신, 기기 의존 검증은 전부 placeholder job(E1~E7 갭 표면화) + E등급 문서 추적 + 수동 절차 문서로 분리하는 구조(`.github/workflows/ci.yml`의 device-evidence-placeholder, `docs/05-tdd-quality-strategy.md:77-168`). 즉 B군은 "코드를 고치는 작업"이 아니라 "절차를 수행하고 증거를 기록하는 작업"이다.

### C. 플랜 문서 기준 미완료

- **`docs/plans/2026-08-25-v0.2-hardening.md`** — 가장 큰 미완 플랜. 체크박스 전부 `[ ]`: HMAC 실기기 검증(109-110행), Windows 물리(146-147행), 60분 soak+자동 재연결(178-179행), release 서명/notarization/v0.2.0 태그(226-228행), 최종 실기기 회귀(272행).
- `docs/plans/2026-08-26-usb-aoap-transport.md` — T11 물리 6단계 미실행으로 기록(1223행)하나 이후 2026-09-03 부분 수행됨(문서 갱신 대상).
- `docs/plans/2026-09-01-usb-display-extension.md` — 구현은 완료·기록됐으나 전부 E3 수준(222행).

## 우선순위 제안 (근거 요약)

1. **미커밋 리서치 커밋** — 비용 5초, 유실 방지, 후속 작업 근거.
2. **T11 잔여(검증 4 soak + 검증 6 앱 UI 경유)** — 절차·합격 기준·진단 가이드까지 문서화 완료(`docs/usb-physical-validation.md`). 끝나면 AOAP·가상 디스플레이의 E3→승격 판정이 가능해지는 현재 병목.
3. **CGVirtualDisplay 스파크** — 오늘 리서치의 미해결 질문 1–2를 해소하는 하루짜리 evidence 수집. 가상 디스플레이가 확인된 "유일한 미완 영역"의 다음 수.
4. **커서 분리 설계+구현** — 참고 구현 확보, 리서치 2건이 일치하는 최상위 격차.
5. **transport-quic 구현 또는 G2 bake-off 계획** — H14 red만 있는 크레이트를 실제로 만들거나, 기기 제약을 문서로 명시하고 보류 선언.

## 코드 참조

- `crates/transport-quic/src/lib.rs:3` — 크레이트 전체 placeholder 선언 (ADR-0004 대기)
- `crates/host-core/src/lib.rs:1,15-17,84` — fake 전용 오케스트레이션
- `apps/host-desktop/src-tauri/Cargo.toml:30-34` — 프로덕션 미연결 크레이트 근거
- `apps/host-desktop/src/App.tsx:946-960` — 가상 디스플레이 옵트인 게이트 (localStorage `leftcar_virtual_display_experiment`)
- `native/macos-capture-shim/Sources/Capture/CaptureSession+Backend.swift:102` — `showsCursor = true` (커서 분리 미달 지점)
- `docs/usb-physical-validation.md:140-155` — 검증 1~6 결과 표 (검증 4 미수행, 검증 6 partial)
- `docs/EVIDENCE.md:60-72` — 대기 중 증거 E4–E7 표 (장치 확보 시 순차 실행)
- `docs/EVIDENCE.md:220-253` — USB 화면 확장 3종 구현 기록 (전부 E3 수준 명시)
- `docs/09-risk-register.md:30` — R-015 (virtual display scope 팽창 Mitigating)
- `docs/plans/2026-08-25-v0.2-hardening.md` — 체크박스 전부 미체크인 유일 플랜

## 아키텍처 인사이트

1. **"미완료"의 표현 방식**: 이 저장소는 TODO 주석 대신 E등급(E0–E7)·L레이어(L1–L6)·옵트인 게이트·ADR 상태로 미완을 추적한다. skip/ignore 테스트가 0건인 것이 그 증거. 따라서 "다음 작업"은 코드 검색이 아니라 EVIDENCE/검증 문서의 등급 표에서 읽어야 한다.
2. **현재 병목은 코드가 아니라 게이트 수행이다**: T11 절차 문서가 완비된 시점에서 남은 것은 물리 실행 + 결과 기록. 자동화로 못 푸는 영역임을 문서가 명시적으로 관리하고 있다.
3. **가상 디스플레이는 "실험 완료, 판단 대기" 상태**: 코드·게이트·실기 CLI 검증까지 끝났고 남은 것은 앱 UI 경유 확인과 CGVirtualDisplay 대안 평가(별도 ADR).

## 히스토리 컨텍스트 (thoughts/ 디렉토리)

leftcar에는 `thoughts/` 디렉토리가 없다. 동일 역할 문서는 전부 `docs/` 아래에 있으며 위 코드 참조에 반영했다.

## 관련 리서치

- `docs/research/2026-09-03_opendisplay-comparison.md` — OpenDisplay 비교 (가상 디스플레이·커서 분리 근거, **현재 untracked**)
- `docs/research/2026-08-25_remote-screen-display-pipelines.md` — 12개 제품 파이프라인 비교 (커서 분리 미달 결론의 선행 문서)

## 미해결 질문

1. transport bake-off(G2)를 Galaxy XR 없이 진행할 수 있는가 — TB710FU로 대체 가능한지, 아니면 QUIC 구현을 먼저 할지 (ADR-0004 확정 경로 결정).
2. 설계 크레이트 8종의 프로덕션 연결이 목표인가 — 현재 native shim 경로가 실기 검증을 통과한 상태에서 "연결"이 실제 이득인지 판단 필요 (ADR 후보).
3. 커서 분리 채널 설계 — UDP 사이드 채널(OpenDisplay 방식) vs 기존 제어 채널(LCS1/LCI1) 확장.
4. v0.2 릴리스 범위 — hardening 플랜의 어디까지를 v0.2.0에 포함할지 (서명/notarization 없이 태그할지).
