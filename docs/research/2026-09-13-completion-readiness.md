---
date: 2026-09-13T21:33:00+09:00
researcher: Codex
git_commit: 068b6628df5dc57f264446f8ec51215b37c51b6f
candidate_commit: a62d90850e03e01ff165f306c7c1a92e36dcd296
branch: main
repository: leftcar
topic: "Leftcar를 마무리하기 위해 앞으로 필요한 일"
tags: [research, release-readiness, streaming, security, ux, device-validation]
status: complete
last_updated: 2026-09-13
last_updated_by: Codex
---

# Leftcar 마무리 조사: 후보 통합부터 실사용·출시까지

> 실행 후속 정보: PR #5는 `5a8dde4`로 머지됐다. 활성 세션에서 전달받은 최신 사용자 결정은 실기기 안정성 기준 30분, 검사는 추후 직접 수행이다. 아래 60분 목표와 당시 PR/CI 상태는 조사 시점의 역사 기록이며 현재 실행 기준은 [지원·완료 표](../completion-and-support.md)를 따른다.

## 판단

**현재 가장 효과적인 다음 작업은 이미 만든 개선을 하나의 후보로 통합하고, 그 후보의 실제 영상 연결과 장시간 사용을 검증하는 것이다.** 성능·복구 기능의 상당 부분은 구현돼 있다. 추가 코덱이나 대규모 구조 변경을 시작하기 전에, 같은 소스·Host·APK에서 “연결 → 표시 → 조작 → 복구 → 종료”를 끝까지 닫아야 한다.

프로젝트의 완료에는 서로 다른 세 단계가 있다.

1. **코드 통합 완료:** 개선 브랜치와 자동 검사, 새 HEAD의 CI까지 일치한다.
2. **실사용 안정판:** 정한 장치·네트워크·프로필에서 실제 영상과 입력, 복구, 장시간 수용 기준을 만족한다.
3. **공개 배포 완료:** 위 후보를 재현 가능하게 패키징·서명하고, 기존 설치에서 업데이트할 수 있으며 지원 범위와 제한을 정확하게 안내한다.

1번이 끝나도 2·3번이 자동으로 완료되지는 않는다. 이번 조사는 제품/UX, 스트리밍/실기기, 보안/배포를 별도로 검토한 뒤 현재 소스로 교차 확인했다. 제품 코드를 수정하거나 기기·네트워크를 조작하지 않았고 이 보고서와 조사 근거만 추가했다.

## 1. 실제로 어느 버전을 조사했는가

| 대상 | 확인한 상태 | 의미 |
| --- | --- | --- |
| 로컬 main | `068b662`, 조사 시작 시 clean | 현재 작업 디렉터리의 기준 |
| 원격 main | 최초 조회 `27e33e4`, 로컬 main보다 20커밋 뒤 | 로컬 검사 결과가 원격 제품에 반영됐다는 뜻은 아님 |
| 개선 브랜치 | `.worktrees/audit-improvements`, 최종 관측 `a62d908`, clean | 로컬 main 이후 3커밋. 통합할 후보 |
| 새 PR | [#5](https://github.com/loopy-lim/leftcar/pull/5), `a62d908`, OPEN | 조사 도중 별도 작업에서 push·PR 생성됨 |
| 개선 브랜치 소스 | build-input SHA-256 `6b73499b…`, 입력 872개 | 기존 패키지 입력과 다름. 새 패키지 증거 필요 |
| 공개 최신 릴리스 | `v0.1.4`, 2026-09-08, Viewer APK 1개 | README의 `v0.1.2`·3종 산출물 설명과 다름 |
| 최초 조회 시 최신 완료 CI | `27e33e4`의 2026-09-09 실행 실패 | Android 타입 검사, Rust fmt, Windows Clippy 실패. 최신 로컬 소스의 실패로 소급하지 않음 |
| 최종 조회한 후보 CI | `a62d908`, [실행 34757386802](https://github.com/loopy-lim/leftcar/actions/runs/34757386802) | Rust/Android 실패, TS 성공, Windows 진행 중 |

조사 중 다른 작업에서 기존 미커밋 5파일을 정리한 `a62d908`이 생겼다. 그 전후 5파일 바이트와 전체 build-input 해시가 같음을 확인했다. 따라서 처음 발견한 “2커밋 + dirty 5개”를 현재 미완료 상태로 반복하지 않는다. 원격 통합은 동시에 진행될 수 있으므로 실행 직전에 PR/CI 상태를 갱신해야 한다.

공개 상태 근거: [v0.1.4](https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4), [확인한 완료 CI](https://github.com/loopy-lim/leftcar/actions/runs/34309139077). 로컬 기준과 명령·로그 해시는 [조사 receipt](2026-09-13-completion-readiness.json)에 기록했다.

### 현재 후보에서 실제로 실패한 CI 두 곳

새 PR의 로그까지 확인했다. 과거 커밋의 실패와 별개로 **현재 후보에도 fresh runner 빌드 준비 순서/도구 누락이 있다.**

- macOS Rust 잡: `Host desktop crate tests`에서 Tauri 리소스인 `native/macos-capture-shim/libleftcar_capture.dylib`가 없어서 build script 실패. shim 생성이 Host 검사보다 뒤에 있다. 해결 방향은 native 리소스를 검사 전에 생성하거나 해당 검사에 필요한 번들 구성을 명시하는 것이다. 로컬 캐시가 있는 검사 통과로 해결됐다고 보지 않는다. [실패 잡](https://github.com/loopy-lim/leftcar/actions/runs/34757386802/job/103724007153).
- Android 잡: `Gradle JVM unit tests`가 Rustra의 `buildRustAndroid`를 실행하면서 `cargo ndk`를 찾지 못했다. `error: no such command: ndk`가 직접 원인이다. CI에 필요한 `cargo-ndk` 설치/버전 확인을 포함하고 clean runner에서 재검증해야 한다. [실패 잡](https://github.com/loopy-lim/leftcar/actions/runs/34757386802/job/103724007173).

이 두 건은 테스트 assertion 실패나 현재 영상 성능 실패로 분류하지 않는다. **현재 PR의 재현 가능한 빌드·검사 문제**이며, 통합 전에 새 실행을 통과시킬 구체 작업이다. 마지막 조회 때 Windows는 진행 중이었으므로 결과를 선판정하지 않는다.

### 이번 조사에서 직접 실행한 검사

| 대상 | 검사 | 결과 |
| --- | --- | --- |
| main | 루트·Viewer·Host 타입 검사 | 통과 |
| main | main만 포함한 Vitest | 50파일 / 612개 통과 |
| main | 계약 / TS·Kotlin 구조 검사 | 4개 통과 / clean |
| main | Rust workspace | 399개 통과, 1개 ignored |
| main | 별도 Tauri Host crate | 단위 160 + 통합 12 통과 |
| main | workspace fmt / Clippy | 통과 |
| main | React Doctor | 100 / 100 |
| 개선 후보 | 타입 검사 / Vitest | 통과 / 57파일·559개 통과 |
| 개선 후보 | React Doctor | 100 / 100 |

**테스트 집계 문제도 확인했다.** 루트 `bun run test`는 중첩 `.worktrees/audit-improvements`까지 발견해 1,171개를 실행했다. 이것을 main의 테스트 개수로 쓰면 잘못이다. `bun run test --exclude '**/.worktrees/**'`로 main의 612개를 별도 확인했다. 계약 4개는 전체 JS 검사에도 들어가므로 합산하지 않는다.

개선 브랜치에는 같은 `6b73499b…` 입력의 `bun run verify all` 통과 기록도 이번 조사 중 추가됐다. 21:24:34–21:30:01 KST, 327.189초, 전후 입력 동일·종료 코드 0이며 UI 78개/9 suite, Host 225+12, JVM 118개/19 suite 등을 기록한다. 이 전체 실행은 **별도 작업의 증거**이고, 이번 조사에서 직접 다시 실행한 후보 검사는 위 세 가지다. [후보 검증 문서](../2026-09-13-audit-remediation-validation.md), [기계 판독 근거](../superpowers/evidence/2026-09-13-audit-validation.json).

## 2. 이미 만든 것을 다시 만들지 않기

| 영역 | 현재 구현 또는 후속 확인 | 남은 일 |
| --- | --- | --- |
| 성능·복구 | split 포트 예약, AU deadline/overflow 처리, NACK/RTX, 타일별 복구, 혼잡 공유, 빠른 bitrate 회복 | 현재 후보의 실측·회귀 확인 |
| 디코더 자원 | admission, 후보의 실제 Activity 수명별 예약·반납 | 다중 창·OEM별 실제 자원 검증 |
| 연결·권한 | 후보의 늦은 연결 취소, 기기별 화면 승인, 입력 기본 OFF, 철회·재시작 정책 | 통합 및 실제 권한 전이 검증 |
| UX | 카메라 영구 거부 복구, 종료 칩, 모달 포커스/Esc, 기기 이름, 오류·취소 표시 | 실제 사용자 흐름·접근성 검증 |
| XR 텍스트·UI | IME, XR feature 선언, Home Space, 비율 프리셋·패널 밀도 | XR 기기에서 입력·가독성·다중 창 확인 |
| 빌드·출처 | 후보의 doctor/verify/build/manifest와 패키지 내부 JS·native 확인 | 통합 뒤 정확한 배포 후보 재빌드 |

9월 11일 성능 리뷰는 앞부분의 미해결 목록 뒤에 같은 날 구현 결과가 붙어 있다. 앞부분만 읽어 다시 구현 목록을 만들면 안 된다. 실제 상태는 `docs/2026-09-11-performance-review.md:232` 이후 및 후보 소스를 기준으로 했다.

9월 7일 오전 인계에 남았던 동일 세션 1440p↔4K 전환도 저녁 후속 APK에서 왕복 성공 기록이 있다(`docs/responsive-streaming-validation.md:78`). 지금은 과거 버그를 다시 확정할 것이 아니라 새 후보에서 회귀를 확인할 일이다.

**가상 디스플레이 생성·관리 기능은 사용자 요청으로 제거됐다.** `docs/virtual-display-removal-validation.md:3`에 제거 범위가 명시돼 있다. 이를 출시 필수 기능으로 되살리지 않는다. 벤치마크용 외부 전용 화면을 쓰는 것과 제품에서 가상 화면을 생성하는 기능은 별개다.

## 3. 지금 막혀 있는 가장 큰 경로: 실제 영상 전달

가장 최근 패키지의 TB710FU 설치, 페어링, 전용 화면 승인, 화면 목록 제한은 기록돼 있다. 그러나 직접 LAN TCP 연결은 시간 초과했고, ADB-over-Wi-Fi의 제어 연결 이후에는 `UDP reachability proof failed for 127.0.0.1:5001`로 영상 시작이 실패했다.

이 기록에서 확인된 것은 제어 연결과 승인이다. **실제 영상 시작, FPS, 지연, 10분·30분 재생 통과는 없다.** 합성 화면을 띄운 70.219초와 패키지 빌드 시간은 재생 시간이 아니다. 이후 simplify된 현재 소스는 패키지를 다시 만들지도 않았으므로 이전 설치 해시를 최신 후보의 설치 증거로 사용할 수 없다. 근거: 후보 검증 문서 `:74–90`, receipt의 `physical`·`simplifyVerification`.

### 가장 먼저 수행할 진단

1. 새 후보의 정확한 Host/APK와 **1080p60 단일 화면**으로 시작 조건을 작게 고정한다.
2. 실제 LAN 경로라면 Host listener의 bind 주소, Viewer의 실제 LAN 주소·route, TCP 도달성을 먼저 확인한다. listener/방화벽/OS 권한/VPN은 점검 후보이지 현재 확정 원인이 아니다.
3. 제어에 성공한 뒤 미디어 후보 주소와 봉인된 UDP 왕복 증명, 네이티브 decoder/render 카운터 증가를 확인한다. loopback 제어 주소를 원격 Viewer의 UDP 주소로 간주하지 않는다.
4. LAN, `adbTcp`, 실제 AOAP USB 중 선택한 경로를 각각 기록한다. ADB reverse가 있다는 사실만으로 유선 미디어 성공이라 부르지 않는다.
5. 시작·정지·재연결을 짧게 3회 성공시킨 뒤 10분 측정을 시작한다.

**USB의 별도 잔여:** 후보 `aoap_proxy.rs:19–55`에서 stop은 flag와 채널을 해제하지만 worker 종료를 기다리지 않는다. 빠른 재시작이 `already active`를 볼 수 있는 수명 경합이 남는다. USB를 이번 지원 범위에 포함하면 종료 완료 동기화와 start/stop·분리/재연결 회귀가 필요하다. 실기기 발생 빈도는 측정하지 않았다.

또한 후보 `launch-stream.ts:333`은 split 모드의 전송을 UDP로 선택한다. 따라서 **4K split + AOAP를 단순히 케이블만 꽂으면 가능한 조합으로 안내하면 안 된다.** 지원할 조합을 정하고 필요한 구현·협상 여부를 먼저 확인한다.

## 4. 완료의 기준을 하나로 맞추기

현재 요구사항은 실제 제품과 맞지 않는 부분이 있다.

| 항목 | 불일치 | 권장 결정 |
| --- | --- | --- |
| 입력 기본값 | PRD P-01/FR-019는 OFF, 중복 FR-021은 자동 활성화. main은 자동, 후보는 OFF | 후보의 화면 승인 + 세션별 입력 승인 정책을 제품 문서·UI·회귀 기준에 일치시키기 |
| source 범위 | PRD/README는 앱 창과 디스플레이, 후보 실제 승인은 디스플레이 | 다음 안정판을 display-only로 명시하거나, 앱 창 캡처를 실제 추가 구현·검증 과제로 남기기 |
| 부가 기능 | 오디오·파일·클립보드가 PRD 비목표인데 현재 구현·노출됨 | 이번에 보장할 기능과 후속/실험 기능을 정하기. 조용히 삭제하거나 자동으로 전부 필수화하지 않기 |
| 플랫폼 | macOS·Windows·Android 일반 기기·XR가 한 제품 설명에 섞임 | OS/장치/전송/코덱/프로필/창 수의 지원 표를 만들기 |
| 성능 목표 | 1080p·1440p·4K 및 짧은 smoke·장시간 기준이 혼재 | 프로필별 필수 수치와 실험 시간을 분리하기 |
| 공개 버전 | README `v0.1.2`, GitHub `v0.1.4`, Viewer 소스 `0.1.5`, Host `0.1.2` | 버전 역할과 Host↔Viewer↔shim 호환성을 한 릴리스 표로 관리하기 |

근거: `docs/01-product-requirements.md:37,69,89,102`, 후보 `docs/07-security-privacy.md:450`, `README.md:7`, `docs/versioning.md:6`.

**추천하는 첫 마감선**은 macOS arm64 + Android arm64의 검증할 특정 기기 + LAN + 승인한 디스플레이 + H.264 기준선이다. 이는 내부 안정화 마일스톤 제안이다. 기존 PRD의 4K60·Galaxy XR 네 창·60분 조건을 만족한 것으로 이름만 바꾸는 제안이 아니다. PRD v1 전체 완료를 원하면 아래 해당 게이트를 그대로 유지해야 한다.

Android 공식 가이드도 일반 모바일 호환, 대화면 호환, XR 특화 경험을 구분한다. Home Space 기반 제품을 마무리하기 위해 Full Space·손 추적·공간 앵커를 모두 새로 만들 필요는 없다. 대화면/다중 창·입력 경험을 실제로 검증하는 것이 먼저다. [Android XR 품질 가이드](https://developer.android.com/docs/quality-guidelines/android-xr).

## 5. 실기기 수용 계획

아래 기존 수치는 **달성값이 아니라 문서에 남아 있는 목표**다. 새 반복 횟수는 권장 실험 설계이며 기존 요구와 구분했다.

| 검사 | 조건과 완료 기준 | 선행 조건 |
| --- | --- | --- |
| 기본 영상 | 1080p60 단일 화면의 실제 변화·decoder/render 증가, start/stop/reconnect 짧게 3회 | 실제 transport 복구 |
| 1440p·4K | 같은 high-motion workload와 동일 후보로 비교. K1은 실제 3840×2160 입력에서 10분 encoder/render 평균 각각 ≥59fps | 기본 영상 |
| 같은 세션 전환 | 1440p→4K split→1440p→4K, resize, 반복 close/open. 권장 3회씩, timeout·잘못된 Surface/포트 재사용·누수 없음 | 지원 transport 조합 확정 |
| 연결 회복 | 네트워크 회복 후 첫 표시 ≤1초. background 30초 뒤 ≤3초 상태 복구. crash/source 종료·Host 재시작·sleep/wake도 별도 | 기본 영상 |
| 입력·권한 | 기본 입력 OFF, Host 허용 후 포인터/휠/드래그/수식키/한글 IME, 철회·창 종료·초점 이탈 뒤 눌린 입력 해제 | 사용자에게 승인된 입력 시험 범위 |
| 다중 창 | 서로 다른 source의 1→2→4창, 비초점 갱신, Hub 종료, 한 창 오류 격리, 실제 codec 자원·예약 반납 | 기본 영상 + 대상 기기 |
| 30분 | 다중 창 메모리 지속 증가 없음, 복구·열·출력 통계 보존 | 짧은/10분 성공 |
| 60분 | 초기 대비 지연 p50 증가 ≤16ms, 반복 크래시·누수 없음. thermal severe 지속 여부 기록 | 30분 성공 |
| 광학 지연 | S1 glass-to-glass p50≤50ms/p95≤80ms. 240fps 카메라·200개 이상 표본, 물리 입력 지연은 별도 집계 | 안정된 기준선 |

기존 목표 근거: `docs/01-product-requirements.md:233–243`, `docs/06-benchmark-device-validation.md:202,275,416`. 권장 순서는 **짧은 반복 → 10분 → 30분 → 60분**이다. 10분/30분 통과를 60분 통과로 대체하지 않는다.

평균 FPS 외에 최장 정지, 저프레임 구간, outputDrops, IDR/NACK/RTX, 손실 이후 첫 출력, 메모리, 온도를 함께 기록한다. 4K split은 어느 타일에서 만든 자극인지 구분한다. 충전 중 배터리 변화로 전력 우위를 주장하지 않고, 3200×2000 물리 패널과 4K 인코딩 입력도 구분한다.

과거 180초/621초의 약 59fps 결과는 유용한 선행 관측이다. 최신 소스·기기·반복 workload·장시간 수용으로 옮겨 적지는 않는다. 과거 문서에는 Galaxy XR 재연결 관측도 있으므로 “XR에서 한 번도 재생된 적 없다”고 단정하지 않는다. **최신 후보의 XR 네 창과 장시간 수용이 부족하다**는 것이 이번 판단이다.

## 6. 후보에도 남은 실제 수정·정책 항목

### 6.1 Viewer 설정 저장 실패 — 다음 안정판 전 수정 권장

후보 `viewer-preferences.ts:133`은 SecureStore 읽기 실패를 기본값으로 반환한다. `use-catalog-model.ts:190–207`은 이 결과로 loaded를 켠 다음 기본값을 저장한다. 일시 읽기 실패가 기존 선택을 덮어쓸 수 있는 코드 경로다. 같은 파일 `:206,250`은 설정·클립보드 쓰기 실패를 삼키므로 화면 표시와 다음 실행 상태가 어긋날 수 있다. 실제 기기 장애를 재현한 결과는 아니다.

완료 기준: 읽기 실패/값 없음 구분, 실패 시 자동 덮어쓰기 금지, 저장 실패의 사용자 표시와 재시도 또는 복원, 프로필/FPS/커서/오디오/클립보드 설정의 앱 재시작 확인. 이미 있는 native 설정 세대 가드는 재작성하지 않는다.

### 6.2 SecureStore 백업 제외 대상 — 다음 안정판 전 수정 권장

후보 Android backup XML은 `ReactNativePreferences.xml`만 제외한다. 실제 Expo SecureStore 구현은 `SecureStore` preference 파일을 사용하며 data-extraction XML에는 device-transfer 제외도 없다. 복원된 암호문이 복호화되지 않아 페어링·설정 복구를 망칠 수 있는 설정 불일치다. 이것만으로 평문 토큰 유출이나 자격 복제가 입증된 것은 아니다.

완료 기준: 최종 병합 manifest/XML에서 cloud-backup과 device-transfer의 SecureStore 제외 확인, 백업/복원 이후 재페어링·설정 복구 동작 검증. [Expo 공식 백업 지침](https://docs.expo.dev/versions/latest/sdk/securestore/#android-auto-backup). 소스: 후보 `android/app/src/main/res/xml/secure_store_backup_rules.xml:3`, `secure_store_data_extraction_rules.xml:4`.

### 6.3 키·로그·화면 프라이버시 — 공개 배포 전 정책과 구현 일치

- Host 정체 키는 `identity.rs:49–70`에서 seed를 JSON 파일로 저장한다(Unix 생성 권한 0600). 문서의 일반 “OS secure storage” 설명과 차이가 있다. Keychain/Credential Manager로 옮길지, 해당 파일 저장을 명시적 위협 모델로 수용할지 결정하고 키 재생성·백업·복구 시 신뢰 변화도 설명한다. 이미 OS 보호 저장소를 사용하는 페어링 토큰과 구분한다.
- 감사 로그는 `audit.rs:39`의 임의 JSON 필드와 `control.rs:2501`의 device/viewer 주소/source, 파일 전송의 파일명을 보존한다. 로컬 감사 목적과 진단 export의 허용 정보를 나누고 allowlist·가명화·회전·보관 기간을 결정한다. 0600 설정만으로 문서의 “IP/이름 미기록” 계약이 성립하지 않는다.
- StreamActivity의 recents preview/화면 캡처 정책, 사용 목적이 확인되지 않은 overlay·legacy storage 권한을 정리한다. `FLAG_SECURE` 적용은 XR/Home Space 표시 호환성을 검증한 뒤 결정한다. `usesCleartextTraffic=true`만으로 별도 AEAD 미디어가 평문이라고 판정하지 않는다.

이미 구현된 AEAD·host key pinning·기기별 source 승인·입력 OFF는 재설계 과제가 아니라 통합·회귀 확인 대상이다. 이번 조사는 침투시험이나 취약점 전수 진단을 수행한 것이 아니다.

## 7. UX·플랫폼·운영에서 닫아야 할 부분

### 첫 사용과 일상 사용

새 사용자 기준 “설치 → 화면 기록 권한 → 페어링 → 화면 승인 → 영상 → 선택적 입력 → 종료”를 개발자 shell 없이 수행하게 한다. 권장 실험은 설명 없이 첫 영상까지 5분 이내이며, 잘못된 코드·카메라 거부·오프라인·권한 철회에서 필요한 다음 행동이 보이는지 관찰한다. 이는 새 목표 제안이지 측정된 성과가 아니다.

Host의 파일 공유 카드가 핵심 화면보다 먼저 상시 노출되는 구조(`App.tsx:1218–1250`)와 세로 고정 Hub를 점검한다. 큰 개편 전에 실제 사용자·XR 패널에서 어려움이 재현되는지 확인한다. 한국어/영어, 큰 글자, TalkBack, 외부 키보드, 종료 칩과 비초점 창의 조작성도 포함한다. 페어링 시스템 알림·트레이 배지는 후속 개선으로 둘 수 있다.

### 지원 플랫폼

- **macOS:** 정확한 서명에서 권한 재사용, 거부→허용→철회, Host 종료/재시작, 디스플레이 교체, sleep/wake를 확인한다.
- **Android:** TB710FU 하나의 성공을 모든 OEM/최소 API/동시 디코더 상한에 일반화하지 않는다. 최소 지원 API와 실제 시험 기기, codec 이름·지원 인스턴스를 명시한다.
- **Galaxy XR:** Home Space 네 창, 비초점 갱신·IME·우클릭/스크롤·비율 전환을 실제 시험한다. 일반 MotionEvent 경로가 있다는 이유만으로 XR 조작 실패 또는 성공을 단정하지 않는다.
- **Windows:** WGC/MFT/SendInput 소스·교차 검사와 실제 Intel/AMD/NVIDIA GPU·DPI·회전·다중 모니터·UIPI·설치 시험을 분리한다. 처음부터 모든 GPU를 보장하기보다 검증된 조합부터 공개한다.
- **USB:** AOAP와 ADB TCP를 다른 전송으로 기록한다. 빠른 재시작 경합과 실제 케이블·권한·분리/재연결 검증을 통과한 조합만 지원으로 표시한다.

### 공개 패키지와 유지보수

1. 후보에 이미 있는 build/manifest 도구를 사용해 source hash, commit, Host/APK/native·JS 해시, 대상 ABI, 실제 서명, 버전과 시간을 한 묶음으로 남긴다. 현재 후보는 v9 shim을 요구하므로 이전 Host 리소스를 섞지 않는다.
2. 최신 태그가 Viewer만 제공하면 사용할 Host 버전과 호환성을 명시한다. 모든 내부 package 버전을 억지로 같게 하기보다 사용자에게 보이는 release/compatibility 계약을 검사한다.
3. Android의 공개 서명 키와 기존 debug 설치의 이전 경로, macOS Developer ID·공증, 공개 Windows 설치판의 서명 방침을 준비한다. 내부 서명과 공개 배포를 구분한다. [Android 앱 서명](https://developer.android.com/studio/publish/app-signing), [Apple Developer ID](https://developer.apple.com/developer-id/).
4. 새 설치와 기존 버전 업데이트에서 앱 데이터·페어링·권한 유지 및 필요한 재승인을 시험한다. 자동 updater가 없어도 명확한 수동 업데이트·문제 버전 회수 절차로 시작할 수 있다.
5. 별도 작업트리가 테스트에 섞이지 않게 test inventory를 고정한다. fresh checkout에서 후보의 doctor/verify/빌드가 성공하는지 확인하고 CI도 같은 source/toolchain을 사용한다.
6. 취약점·라이선스 검사와 release dependency 목록을 준비한다. Actions 참조·Gradle 배포 checksum의 고정, SBOM/attestation은 공개 배포 규모에 맞춰 보강한다. React Doctor는 저장소 규칙대로 `latest`/100점을 유지하고 실제 실행된 버전을 receipt에 남긴다.
7. README·문서 인덱스·PRD·risk register·EVIDENCE의 상태/날짜를 맞춘다. EVIDENCE의 오래된 crate·테스트 이름을 현재 소스와 대조하고, 과거 기록과 현재 지원 표를 분리한다.

## 8. 권장 실행 순서와 작업 단위

P0는 다음 단계를 막는 일, P1은 안정판/공개 배포 전에 닫을 일, P2는 지원 범위와 실측 결과에 따라 진행할 일이다. 역할은 다음 작업의 권장 담당 구분이며 새 에이전트 실행이나 사용자 승인을 의미하지 않는다.

| 순서 | 우선 | 작업 | 담당/의존 | 완료 산출물 |
| --- | --- | --- | --- | --- |
| 1 | P0 | 제품 완료 범위·지원 조합과 요구 충돌 정리 | 제품/QA, 즉시 가능 | 한 장의 scope·acceptance 표 |
| 2 | P0 | PR #5의 shim 준비·cargo-ndk 누락 수정, 새 HEAD CI 통과 후 통합 | 통합 담당, 기존 진행 작업과 중복 방지 | PR/CI/commit 연결 |
| 3 | P1 | 설정 저장·백업 제외·지원할 USB 수명 경합 수정 | Viewer/Host, 후보 기준 | 재현 회귀·변경 범위별 검사 |
| 4 | P0 | 정확한 Host/APK 재빌드·검증·설치 후보 고정 | 빌드/QA, 2·3 이후 | source/package/설치 receipt |
| 5 | P0 | 실제 제어·미디어 경로 진단과 단일 영상 3회 | 네트워크/기기, 4 이후 | 실제 decoder/render 증가·전송 종류 |
| 6 | P1 | 권한·입력·전환·복구·다중 창 매트릭스 | 기기 QA, 5 이후 | 시나리오별 성공/실패·원본 로그 |
| 7 | P1 | 10→30→60분·광학 지연·4K 조건 검증 | 성능/기기, 5·6 이후 | 조건·해시·정지/열/메모리 포함 결과 |
| 8 | P1 | 키·감사/화면 프라이버시 정책과 첫 사용자 UX | 보안/UX, 병행 가능 | 위협 모델·정책·사용성 검증 |
| 9 | P1 | 서명·업데이트·지원표·문서·release gate | 릴리스, 실제 지원 조건 확정 후 | 설치/업데이트 가능한 검증된 배포 묶음 |
| 10 | P2 | Windows/XR/OEM/USB 지원 범위 확대 | 해당 플랫폼 담당 | 조합별 별도 수용 결과 |

**가장 먼저 닫을 작은 목표:** 현재 후보의 1080p60 단일 화면을 실제 transport로 3회 연결하고, 10분 동안 유효한 영상·복구 통계를 얻는 것. 이어서 목표 프로필과 창 수를 늘린다. 구체 일정은 현재 LAN 실패 원인과 기기 가용성이 미확정이므로 날짜를 확약하지 않는다.

## 9. 지금 추가로 시작하지 않을 작업

N-타일 렌더러 수렴, LTR·HEVC/AV1 실험, 새로운 수신자 주도 페이서, Full Space·공간 앵커, Linux/클라우드 relay/계정 서비스는 기준선 실패의 원인이 해당 구조로 좁혀지거나 별도 제품 수요가 확인될 때 진행한다. Opus·balanced 표시도 현재 기본 OFF인 실험 상태에서 실측 후 승격한다.

가상 화면 생성·관리 기능의 복원은 이번 완료 과제에 넣지 않는다. 과거 삭제 요청과 현재 scope를 존중한다. 이미 구현된 일반 오디오·파일·클립보드도 이번 조사에서 제거하지 않았으며, 릴리스에서 보장할 범위를 먼저 정하도록 남겼다.

## 10. 후속 실행자가 확인할 근거

| 근거 | 확인할 내용 |
| --- | --- |
| `docs/research/2026-09-13-completion-readiness.json` | 이번 조사 명령·결과·로그 해시·소스 구분 |
| `.worktrees/audit-improvements/docs/2026-09-13-audit-remediation-validation.md` | 최신 자동 검증, 과거 패키지, 실패한 실기 미디어, 통합 상태 |
| `.worktrees/audit-improvements/docs/superpowers/evidence/2026-09-13-audit-validation.json` | `simplifyVerification`, source/package hash, `physical` 미실행 경계 |
| `.worktrees/audit-improvements/tools/verify.mjs:18` | 후보의 실제 검사 범위, Swift compile/실행 구분 |
| `.worktrees/audit-improvements/tools/release-manifest.mjs:29` | 소스/대상/산출물 검증 계약 |
| `docs/01-product-requirements.md:233` | 기존 성능·복구·장시간 목표 |
| `docs/2026-09-11-performance-review.md:232` | 과거 미해결과 후속 구현 완료 구분 |
| `docs/responsive-streaming-validation.md:78` | 과거 동일 세션 왕복 전환 성공 |
| `docs/virtual-display-removal-validation.md:3` | 사용자 요청에 따른 기능 제거와 과거 XR 관측 |
| `docs/windows-remote-host.md:75`, `docs/usb-physical-validation.md:149` | 플랫폼별 실제 수용 조건 |

이 보고서의 완료는 **조사 완료**를 뜻한다. 코드 통합·새 패키지·실기기 수용·공개 배포를 수행했다는 뜻이 아니다. 원격 상태와 후보 커밋은 변할 수 있으므로 실제 실행 시 새 snapshot을 만들고 최신 결과로 갱신한다.
