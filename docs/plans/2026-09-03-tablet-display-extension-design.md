# 설계: 태블릿 화면 확장 (덮개 닫힘 유일 화면 모드 포함)

- 날짜: 2026-09-03
- 상태: 승인 (브레인스토밍 세션에서 사용자 승인)
- 관련: ADR-0003 (창 스트림 우선), ADR-0005 (BetterDisplay CLI 옵트인), R-015 (가상 디스플레이 논골), `docs/research/2026-09-03_cgvd-spark-results.md`

## 목표

태블릿이 연결되면 가상 디스플레이를 만들어 새 화면 공간으로 쓴다. 덮개를 닫으면 태블릿이 유일 화면이 되는 "유일 화면 모드"를 지원한다(Sidecar와 동일 시나리오). 입력은 외장 USB 키보드/마우스(네이티브)와 태블릿 터치(역방향 주입)를 모두 지원한다.

## 근거 (스파크 실측, 2026-09-03)

- CGVirtualDisplay private API 4종은 macOS 26.6.2에 생존.
- **활성 화면 0개(헤드리스)에서는 가상 디스플레이 생성 불가** — Sidecar/BetterDisplay/Duet 전부 "만들고 닫기" 구조.
- 따라서 본 기능은 활성 화면 존재 시 VD를 먼저 생성하고, 이후 덮개가 닫혀도 VD가 외부 출력으로 생존하는 흐름이다.

## 아키텍처

```
[활성 화면 ≥ 1 — 덮개 열림 또는 외장 모니터 켜짐]
 1. 태블릿 연결 (USB/Wi-Fi)
 2. 모드 시작: VD 생성 → VD 스트리밍 시작 → caffeinate assertion 획득
 3. 덮개 닫힘 → 내장 패널 꺼짐, VD가 외부 출력으로 생존
    → macOS 세션 유지, 외장 USB 키보드/마우스 네이티브 동작
 4. 태블릿 = 유일 화면. 터치 입력은 LCI1 역방향 주입 (기존 경로)
[종료] 중지 또는 연결 해제 → VD 제거 + assertion 해제
```

| 요소 | 담당 | 상태 |
|---|---|---|
| VD 생성/제거 | `virtual_display.rs` → 프로바이더 추상화 | 추상화 신규, BD 경로 기존 |
| 절전 방어 | `power_assertion.rs` (caffeinate 자식 프로세스) | 신규 |
| 영상 스트리밍 | 기존 SCK→VT→AOAP/UDP 경로 | 기존 무수정 |
| 외장 USB 키보드/마우스 | macOS 네이티브 (코드 불필요) | 없음 |
| 태블릿 터치 입력 | LCI1 (CaptureSession+Input.swift) | 기존, 검증만 |
| UI | "태블릿 화면 확장" 카드로 확장 | 확장 |

## 프로바이더 추상화

```rust
pub trait VirtualDisplayProvider {
    fn name(&self) -> &'static str;              // "betterdisplay" | "cgvirtualdisplay"
    fn available(&self) -> ProviderAvailability; // 설치/API 생존 + 활성 화면 전제
    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError>;
    fn remove(&self, handle: &VirtualDisplay) -> Result<(), ProviderError>;
}

pub struct DisplaySpec { pub name: String, pub width: u32, pub height: u32 }

pub enum ProviderError {
    NoActiveDisplay,          // "덮개를 열거나 모니터를 켜주세요"
    EngineUnavailable(String),// BD 미설치 / CGVD API 부재
    EngineFailed(String),     // abort / displayID=0 등 엔진 내부 실패
}
```

- `BetterDisplayProvider`: 기존 `run_cli`/argv 빌더 이전.
- `CgvdProvider`: 스파크 `tools/cgvd-spark/CGVD.h` 선언 참고의 Swift shim 프로세스 호출. **R-015 논골 유지 — 실험 플래그 뒤에만 존재, 기본 승격은 별도 ADR.**
- 선택 규칙: 기본 `betterdisplay`, CGVD는 localStorage 실험 플래그 옵트인.
- 진입 시 `CGGetActiveDisplayList`로 활성 화면 검사, 0이면 생성 시도 전 `NoActiveDisplay`.

## 절전 방어

- 1차: `caffeinate -s` 자식 프로세스 (AC 전원). 배터리에서는 `-i`로 강등 + UI 경고.
- 2차(옵트인, 기본 꺼짐): 관리자 1회 `pmset disablesleep` 헬퍼.
- 앱 종료 시 caffeinate 자식도 종료되어 assertion 자동 해제 — 누수 없음.

## 상태 머신 (`clamshell_mode.rs`)

```
Idle ──start()──> Creating ──성공──> Streaming ──덮개 닫힘──> ClamshellActive
                     │                │                         │
                     ▼ 실패           ▼ 중지/끊김                ▼ 중지/끊김
                  Failed ◄───────────┴─────────────────────────┘
                                        (정리: VD 제거 + assertion 해제)
```

- 덮개 감지: `ioreg -r -k AppleClamshellState` 파싱 — UI 표시용만, 동작 분기 아님.
- `Drop for ClamshellModeSession` 가드로 모든 경로가 정리를 지나게 함 (Windows `InputInjector` Drop 패턴 준수).
- 자동 재시도 없음 — 원인 3분류를 UI로 표시하고 사용자 재시도.

## UI 이름

- 기능(카드) 이름: **태블릿 화면 확장**
- 덮개 닫힘 생존 상태 문구: **유일 화면 모드**
- 상태 표시: 스트리밍 중 / 유일 화면 모드 / 실패(원인). 시작 버튼은 생성+스트리밍+assertion을 한 번에.

## 검증·테스트 전략

단위(CI): argv 빌더 기존 테스트 유지 + DisplaySpec 검증, NoActiveDisplay 판정, 상태 머신 전이와 drop 가드, caffeinate argv·배터리 강등 분기.

실기(사람 직접, 절차 문서화):
1. 확장 모니터: 덮개 열림 → 시작 → VD 생성 → 태블릿 렌더 (= T11 검증 6 잔여 소거)
2. 유일 화면 모드: 1 중 덮개 닫기 → VD 생존 + 스트리밍 지속 + USB 키보드/마우스 동작
3. 태블릿 터치: `inputEnabled` 옵트인 → LCI1 주입이 VD 위에서 동작 (EVIDENCE.md:158 갭)
4. 절전 방어: 덮개 닫힘 10분 후 세션 생존 + assertion 확인
5. 절차·합격 기준·진단 가이드는 `docs/tablet-display-physical-validation.md`(정식 절차)를 따르고, 결과는 `docs/EVIDENCE.md`에 E등급 기록

React 게이트: UI 변경 후 `npx -y react-doctor@latest . --verbose` 100/100.

## 의도적 제외 (YAGNI)

자동 재시도, 다중 태블릿, Windows 지원, CGVD 기본 프로바이더 승격(별도 ADR 대상).
