# Wi-Fi KM 기반 통합 검증 — 안전 계약 정렬과 전체 목표 격차 (foundation-integration)

- 날짜: 2026-10-04
- 판정: **PASS (기반 계약 정렬 검증 한정)** — `full_goal_satisfied=false`, `wireless_adb_used=false`, `physical_km_test_passed=false`
- 근거 문서: `docs/superpowers/specs/2026-10-03-wifi-km-optional-usb-roadmap.json`, `docs/research/2026-10-03-wifi-km-connected-device-validation.json`

## 1. 검증 대상과 방법

이전 작업(km-foundation)의 결과를 **독립적으로** 재검증했다. 소스를 직접 읽고 계약 항목별로
양쪽 기반을 대조했으며, 테스트는 캐시에 의존하지 않고 `--rerun-tasks`로 신규 실행해 XML 결과를
직접 파싱했다.

- Rust: `crates/domain/src/km.rs` (KM 상태 기계·경계 기하), `crates/domain/src/lib.rs`
- Android: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/` 4개 파일
- 테스트: `crates/domain` 46개, `dev.leftcar.viewer.km.*` 30개 (4 스위트)

## 2. 계약 정렬 매트릭스 (모두 ALIGNED)

| 계약 | Rust 도메인 증거 | Android 증거 | 판정 |
|---|---|---|---|
| 기본 OFF | `KmStateMachine::new` → Off, `KmConsent`/`KmCapabilities` 전부 false 기본 (테스트 `machine_starts_off_and_default_capabilities_are_fail_closed`) | 컨트롤러 5게이트(API34+/ATTACHED_READY/동의/인증/소유권) 전부 false 기본, `desiredMask()`=0 (테스트 `default state is off and emits nothing`, `each missing gate keeps mask zero`, `below api 34 never activates even with every gate satisfied`) | 정렬 |
| 동의 | Arm에 양방향 동의 필수(`MissingBidirectionalConsent`), 활성화 시 방별 재검사 | `SessionConsent` 게이트, 정상 철회→해제, fail-closed→동의 폐기(재연결만으론 재활성 불가) | 정렬 |
| Capability | preflight 전부 기본 false, virtual cursor는 무선 ADB 미승인으로 유지, motion observation은 candidate 명시 | `KmMotionSources.MIN_API=34` 게이트, attach API 34 미만 거부, 중복 결합 거부 | 정렬 |
| 소유권 배타 | I1 — 단일 방향, `ConflictingActiveDirection` 거부 + 상태 브로드캐스트 | `OwnershipActive` 게이트 — 소유권 없으면 구독 0 | 정렬 |
| Fail-closed 순서 | I3 — 모든 Off 경로에서 `ReleaseAllInjectedInput` 최우선 (테스트 `exit_paths_always_release_all_first_and_land_off`, 7경로) | `failClosed()`가 `ReleaseSources`를 사유 보고보다 선행, 멱등 | 정렬 |
| Release-all | 해제 → 서브시스템 정지 → `RestoreLocalInput` 순서 계약 | ServiceInfo read-modify-write로 `motionEventSources=0`, 타 필드 보존, 쓰기 실패 시 재시도 1회+재귀 없음 | 정렬 |
| Touchscreen 비캡처 | virtual-cursor 트리거 capability 게이트 | `MOUSE_FAMILY` 3소스만, `isMouseFamily`·`sanitize` 방어, 클래스 비트 단독 금지, 키 필터 미개수 (테스트 `mask contains no touchscreen device bits` 등) | 정렬 |
| 재연결 무자동재개 | I4 — `Reconnected` 무효 (테스트 `no_event_sequence_auto_activates_after_reconnect`) | fail-closed 동의 폐기로 재연결 단독 재활성 불가 | 정렬 |

sink 방어(어댑터): `onMotionEvent`는 captureActive 게이트 → null-안전 정규화 → 마우스 계열
필터 → sink 예외 시 즉시 fail-closed. 이 역할을 도메인 I3 fail-closed와 대응된다.

## 3. 금지 계약 준수 확인 (소스 증거)

- `AndroidManifest.xml`에서 `accessibility`/`km` 매칭 0건 — 접근성 서비스 미선언, 자동 설치 없음.
- km 패키지 밖에서 `KmAccessibilityMotionAdapter`/`KmCaptureController` 참조 0건 — 자동 배선 없음.
- 무선 ADB·root·debugging 사용 흔적 없음. USB는 roadmap상 명시적으로 미래 확장(미구현).

## 4. 검증 실행 결과 (정확한 명령과 수치)

```
cargo test -p domain
→ 46 passed; 0 failed (0.02s)

cd apps/viewer-expo/android && ./gradlew :app:testDebugUnitTest \
  --tests 'dev.leftcar.viewer.km.*' :app:compileDebugKotlin
→ BUILD SUCCESSFUL

--rerun-tasks 신규 실행 (XML 파싱):
KmAccessibilityMotionAdapterTest  tests=10 failures=0 errors=0
KmCaptureControllerTest           tests=10 failures=0 errors=0
KmMotionSampleTest                tests=5  failures=0 errors=0
KmMotionSourcesTest               tests=5  failures=0 errors=0
합계 30 tests, 0 failures
```

## 5. 미해결 원래 목표 기준 (전체 완료 아님)

1. **라이브 직접 Wi-Fi 세션**: LAN 도달성·mDNS 상호 발견만 입증됨. 동의된 앱 간 Leftcar
   세션은 미개설 — 태블릿 UI를 에이전트가 구동할 수단이 없고 무선 ADB는 금지.
2. **Mac event-tap 권위**: TCC Input Monitoring 미승인 → Mac 경계 감시·억제 탭(Stage 2)
   preflight의 실제 통과 전제 미충족. 도메인 `mac_to_android` capability 게이트는 실사용에서
   아직 닫혀 있음.
3. **Android 실제 커서 주입**: API 34+ `setMotionEventSources` 실기기 적용·해제와 물리 마우스
   이벤트 도달은 미실측(Robolectric 시뮬레이션만). OEM이 motion 이벤트를 캡처·억제할 가능성
   포함(candidate_untested).
4. **양방향 물리 edge-crossing**: mouse/keyboard/real_cursor_hover/edge_crossing/
   global_capture_suppression 전 항목 미수행.
5. **서비스 배선**: onMotionEvent 오버라이드 위임·라이프사이클→dispatch 전달·manifest 등록은
   미구현. 자동 설치 금지 계약상 사용자 명시 단계에서 별도 결정하는 의도된 gap.

## 6. 다음 통합 단계

1. 사용자 명시: 태블릿 Viewer로 leftcar-host 접속해 라이브 세션 개설(전송 재현, 권한 변경 없음).
2. 사용자 명시: Mac Input Monitoring(TCC) 승인 검토 — Stage 2 preflight 실측 조건.
3. 서비스 서브클래스 배선 설계(별도 결정): 게이트 입력 전달과 manifest 선언을 사용자 동의 절차와 함께.
4. 실기기(API 36 TB710FU)에서 setMotionEventSources 실측 — `tools/wifi-km-device-probe.py` 활용.
5. 양방향 물리 edge-crossing·release-all 실측을 도메인 시나리오(Escape/Disconnect/StopAll)와 대응 기록.

## 7. 결론

두 기반의 안전 계약(기본 OFF·동의·capability·배타 소유권·fail-closed 해제 선행·release-all·
touchscreen 비캡처·재연결 무자동재개)은 상호 정렬되어 있고 테스트로 잠겨 있다. 그러나 라이브
연결, Mac 입력 권한, Android 실기기 커서 주입, 양방향 물리 경계 이동은 여전히 unresolved이며,
본 문서는 기반 정렬 검증만을 PASS로 기록한다. 전체 목표 달성을 주장하지 않는다.
