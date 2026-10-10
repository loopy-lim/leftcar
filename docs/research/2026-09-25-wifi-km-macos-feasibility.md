# Wi-Fi 양방향 KM — macOS 측 타당성 조사 (mac-feasibility)

문서 상태: 조사 결과. 기준일 2026-09-25. 이 문서는 구현을 승인받기 전 설계 입력이며, 물리 실기기 검증을 대신하지 않는다. 근거 JSON: [2026-09-25-wifi-km-macos-evidence.json](2026-09-25-wifi-km-macos-evidence.json).

## 1. 질문과 범위

사용자 의도는 Universal Control/Synergy 수준의 양방향 Mac ↔ Android 마우스·키보드 공유다. 이 과제는 그중 macOS 측만 다룬다:

- Mac이 "제어받는 쪽"일 때: 물리 입력을 전역으로 관찰(경계 이탈 감지)하고, 전환 중 억제할 수 있는가.
- Mac이 "제어하는 쪽"일 때: 태블릿에서 온 입력을 전역으로 주입할 수 있는가(이미 부분 구현됨).
- 기존 Wi-Fi 전송·보안·동의 계약을 KM에 재사용할 수 있는가.

Android 측 전역 입력 캡처는 별도 조사 과제다. USB/ADB/root는 전제로 삼지 않는다.

## 2. 검사한 저장소 경로

- `README.md` — 고정 범위, 보안 요약
- `crates/host-core/src/lib.rs` — 호스트 코어는 fake 경로 검증용. 실 입력 경로는 네이티브 shim에 있음을 확인
- `docs/03-system-architecture.md`, `docs/07-security-privacy.md` — 평면 구성, 권한·동의 모델(§21)
- `docs/platform-permissions-audit.md` — macOS 손쉬운 사용/화면 녹화 권한 경계
- `native/macos-capture-shim/Sources/Transport/CaptureSession+Input.swift` — 주입·좌표 매핑·릴리즈 전체
- `native/macos-capture-shim/Sources/Transport/CaptureSession+Cursor.swift` — listenOnly 커서 탭
- `native/macos-capture-shim/Sources/Support/SleepAssertion.swift` — 절전 방지 어설션
- `crates/secure-channel/src/lib.rs`, `crates/control-contract/src/host.rs` — Ed25519/ChaCha20-Poly1305/HKDF 채널

## 3. 결론 요약

macOS 측만 보면 **양방향 KM의 양쪽 방향 모두 플랫폼 수준에서 가능**하다. 주입 방향은 이미 프로덕션 코드로 동작 중이고, 캡처/억제 방향은 공개 API(CGEventTap defaultTap)로 커버된다. 진짜 리스크는 권한이 아니라 **KM 전용 상태 기계**(경계 전환, 자기 이벤트 구분, 보안 입력·잠금 상태)에 있다. Android 측 타당성이 확정되기 전까지 전체 기능의 실현 경로는 미확정이다.

## 4. 항목별 평가

### 4.1 전역 캡처와 억제 (Mac이 제어받는 쪽)

CGEventTap을 `cghidEventTap` + `defaultTap`으로 만들면 시스템 전역 키·마우스 이벤트를 콜백에서 보고, NULL 반환으로 이벤트를 소비(억제)할 수 있다(Apple CGEventTap 문서). 요구 권한은 손쉬운 사용(Accessibility)이며 10.15+에서는 이벤트 관찰에 입력 모니터링(Input Monitoring)이 추가된다. Leftcar는 이미 입력 권한 미승인을 fail-closed 오류로 처리한다(`CaptureShim+Exports.swift:262`, `docs/platform-permissions-audit.md`).

단, 현재 저장소에 억제 경로는 없다. 커서 탭은 `listenOnly`로 설치되어 관찰만 한다(`CaptureSession+Cursor.swift:159-176`). KM "독점 소유"는 새 defaultTap 경로를 의미하며, 억제 중 실종 이벤트 처리와 보안 입력(Secure Event Input) 상호작용은 설계·검증 대상이다.

### 4.2 전역 주입 (Mac이 제어하는 쪽)

이미 구현돼 있다. 포인터는 세션 탭으로(`injectPointerMove`), 키는 HID 탭으로(`injectKey`), IME 완성 텍스트는 유니코드 스트링 이벤트로(`injectText`) 주입한다. 코드 주석에는 macOS 26에서 HID 탭 합성 버튼·스크롤이 드롭되고 세션 탭은 전달된다는 실측 기록이 있어, KM 전환 경로는 세션 탭을 기본으로 삼아야 한다. 한계: Secure Event Input 활성(비밀번호 필드, 로그인 창) 중 합성 키가 전달되지 않는 것은 알려진 플랫폼 동작이며, KM이 잠긴 Mac으로 전환하는 상황에서 이벤트가 무시될 수 있다.

### 4.3 좌표 매핑 (화면 경계)

`pointerPosition`/`currentInputBounds`는 CGDisplayBounds의 글로벌 좌표계(음수 원점 포함)와 0–65535 정규화 매핑, 디스플레이 이동·재시작 재조회를 이미 처리한다. 경계(edge) 전환 KM에는 여기에 더해: 전체 데스크톱(CGDisplayBounds union) 외곽 경계 추적, 전환 시 뷰어 정규화 좌표 기준점 재설정, multi-display 음수 원점 배치가 필요하다. 재료는 검증됐고 결합 설계가 남는다.

### 4.4 전송·보안 재사용

기존 계약이 KM에 그대로 맞는다:

- 제어 평면: QR로 핀된 호스트 Ed25519 정체 키 핸드셰이크 → ChaCha20-Poly1305(RFC 8439) AEAD(`crates/secure-channel/src/lib.rs`)
- 입력 평면: 세션 난수 UDP 검증, latest-wins 포인터(kind 1), 시퀀스/ACK reliable 키·버튼·휠(kind 2–4), release-all 실패 안전(kind 5), 입력 상태 브로드캐스트(LCS1) — `CaptureSession+Input.swift`
- KM 반대 방향(Mac→태블릿)은 같은 reliable 계약의 역방향 재사용이며, Android 측 수용 가능성은 본 과제 범위 밖이다.

### 4.5 피어 입력 동의

§21 계약 재사용: 세션별 입력 기본 OFF, 기기별 상시 승인(재검토 규율), OS 입력 권한 실패 시 잠금 유지, 승인 변경 감사 로그. KM은 "전환된 방향"마다 동의를 따로 요구하도록 이 계약을 확장하는 것이 자연스럽다(설계 단계 결정).

### 4.6 피드백 루프 방지

절반은 완료: 커서 탭이 listenOnly라 물리·합성 이동을 소비하지 않는다. 남은 절반: 억제 탭이나 관찰 탭이 **자기 합성 이벤트**를 물리 이벤트와 구분해야 KM 전환 중 루프가 생기지 않는다. CGEvent의 소스 프로세스 ID/state 필드로 구분하는 것이 표준 방법이며 미구현 — 구현 단계 검증 항목.

### 4.7 배타적 소유권

Mac이 제어받는 동안 Mac의 물리 키보드·마우스를 무시(억제)하는 것은 4.1의 defaultTap으로 가능하나, 이는 사용자가 로컬 입력을 잃는다는 뜻이므로 Synergy/Universal Control 관례(전환 해제 조건, 명시적 탈출 키)를 설계에 포함해야 한다. 대안: 억제 없이 단순히 뷰어 입력을 우선 주입 — 구현은 저렴하지만 커서 충돌이 남는다. 트레이드오프는 구현 계획에서 결정한다.

### 4.8 릴리즈·복구

재료 존재: 세션 종료/입력 OFF 시 `releaseInjectedInput`으로 눌린 키·버튼 전량 해제, release-all(kind 5)로 유실된 상태 재동기화, `SleepAssertion`으로 스트리밍 중 절전 방지. 미검증: 잠금→해제, 디스플레이 구성 변경, 비정상 종료 직후의 KM 시나리오 전체 복구는 실기기 검증이 필요하다.

## 5. 명시적으로 수행하지 않은 물리 검증

- 손쉬운 사용·입력 모니터링 승인 상태에서 defaultTap 억제 + 주입 동시 동작 확인
- macOS 15+ 손쉬운 사용 주기적 재승인 프롬프트가 KM 세션에 미치는 영향
- Secure Event Input/잠금 상태에서의 억제 탭 생존과 합성 이벤트 무시 재현
- 경계 전환 레이턴시와 Wi-Fi(AP 격리 포함) 환경 입력 사용감 실측

## 6. 미확정 사항과 다음 단계

1. Android 측 전역 입력 캡처 타당성(별도 과제)이 전체 경로를 결정한다.
2. 구현 승인 시 KM 상태 기계(방향 전환, 자기 이벤트 필터, 억제 정책, 동의 확장) 설계가 선행되어야 한다. 기존 LCI1/LCS1 계약 위에 얇게 얹는 방향을 권한다 — 전송·보안·동의 계약은 이미 검증됐기 때문이다.
