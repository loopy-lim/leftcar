# macOS 확장 디스플레이(네이티브 가상 디스플레이) 설계 — 기존 구조 대조와 최소 변경안

날짜: 2026-09-18
상태: 구현 완료(미커밋) — v1 실측 결과 §11. 남은 검증: E-4(수면/복귀), E-7(가상 디스플레이 스트리밍+입력), 뷰어 신규 APK(getDisplayMetrics)
선행 문서: `docs/virtual-display-removal-validation.md`(2026-09-07 제거 기록), `docs/decisions/0005-virtual-display-via-betterdisplay-cli.md`(폐기됨), `docs/research/2026-09-03_cgvd-spark-results.md`(macOS 26.6.2 실측), `docs/tablet-display-physical-validation.md`(미실행 체크리스트)

---

## 0. 배경과 재확정

2026-09-07 f1a846e에서 가상 디스플레이 기능 전체(BetterDisplay CLI 엔진 + cgvd-shim 서브프로세스 엔진, 10개 Tauri 명령, 계약 필드 `viewerDisplay`/`virtualDisplayId`/`ResizeVirtualDisplay*`, 뷰어 카드, cgvd 도구)를 제거했다. 제거 당시 "계속 사용하면 느려지는" 원인은 ABR 1-way 래치 3건으로 별도 진단·수정되었고, 가상 디스플레이 자체가 원인은 아니었다.

본 문서는 2026-09-18 사용자 요청(네이티브 private API 기반, BetterDisplay 없이, 기존 파이프라인 재사용)에 따라 **새로 설계**한다. 이전 제거 결정과 배반 관계이므로 다음 원칙으로 갈등을 해소한다:

- BetterDisplay 의존은 **영구 폐기**(ADR-0005 폐기 유지).
- 네이티브 CGVirtualDisplay 경로는 **이번 요청으로 부활**하되, 옛 실패 요인(미실물 검증, UI/옵션 확장, 자동 매칭·리사이즈 결합)을 반복하지 않는다.
- 제거 문서의 검증 체크리스트(`tablet-display-physical-validation.md`)는 이번에 반드시 실행한다.

---

## 1. 현재 구조 분석 — 결론 (file:line 근거)

### 1.1 가상 디스플레이는 "새 source"가 아니라 "그냥 디스플레이"로 떨어진다

핵심 발견: **생성된 가상 디스플레이는 실제 `CGDirectDisplayID`를 갖는 활성 디스플레이**이므로, 기존 경로가 전부 무수정으로 동작한다.

| 단계 | 현재 구현 | 가상 디스플레이 시 |
|---|---|---|
| 열거 | `CGGetActiveDisplayList` → `sortedActiveDisplayIDs()` (`native/macos-capture-shim/Sources/Capture/CaptureBackend.swift:179-195`) | 자동 포함 |
| 안정 ID | `CGDisplayCreateUUIDFromDisplayID` → `macos:display:<uuid>` (`CaptureBackend.swift:197-201`) | 자동 부여 |
| 해상도 | `nativePixelSize(for:)` 최대 backing 모드 (`CaptureBackend.swift:121-176`) | 모드 테이블이 정상이면 자동 |
| 승인 | `GrantStore.allows(owner, source)` — source_id 문자열 키 (`apps/host-desktop/src-tauri/src/source_grants.rs:169-205`) | 문자열 하나 추가로 끝 |
| 캡처 | SCContentFilter + SCStream (`CaptureSession+Backend.swift:76-200`) | 동일 경로 |
| 입력 | 16비트 정규화 좌표 → `inputBounds`(= `filter.contentRect`, `CaptureSession+Backend.swift:79-83`) 선형 매핑 (`CaptureSession+Input.swift:126-134`) | 전역 데스크톱 좌표에 자동 정렬 |
| 소실 처리 | `markStopped` → LCT1 통지 → 호스트 5s 터미널 리퍼 (`CaptureSession+Lifecycle.swift:19-73`, `control.rs:1610-1745`, `TERMINAL_SESSION_RETENTION` control.rs:323) | 디스플레이 제거 시 동일 |
| 스트림 모양 | 뷰어가 `startStream`에 w/h/fps 제시, 호스트 검증(≤8192, ≤90fps, control.rs:119-124) 후 SCK가 소스를 요청 크기로 스케일 | HiDPI backing 소스도 동일 |

즉 제안 문서의 최상위 제약 — "Virtual Display → 기존 Leftcar source → 기존 pipeline" — 은 **카탈로그에 나타나기만 하면 이미 충족된 구조**다.

### 1.2 유일한 갭 3가지

1. **카탈로그 이름이 제네릭**: `"name": "Display \(index)"` (`CaptureBackend.swift:213`). 가상 디스플레이를 "Leftcar Display"로 식별하려면 이름 오버라이드가 필요(§4.3).
2. **커넥터 없음 표시 부재**: 어떤 source가 가상 디스플레이인지 wire `DisplayInfo`(`crates/control-contract/src/host.rs:257-266`)에는 구분 필드가 없다. v1은 불필요(호스트 UI가 상태를 이미 알고 있음).
3. **grants는 생성 시점 이후 추가 필요**: 쌍연결된 기기의 source_ids는 고정 리스트라, 나중에 만든 가상 디스플레이는 자동 노출되지 않는다. 생성 UX에서 grants 연동 필요(§4.6). ⚠️ 현재 다른 세션의 진행 중 변경이 grants UI(SourceGrantEditor 삭제, 승인 시 전체 디스플레이 자동 허용)를 다시 만들고 있어 **병합 시 조율 필요**.

### 1.3 이전 구현의 유산 (git `f1a846e^`에서 회수)

- `tools/cgvd-shim/Sources/cgvd-shim/include/CGVD.h` — private API 시그니처 전체. **그대로 재사용 가능.**
- 실측 레시피(cgvd-spark, 2026-09-03 macOS 26.6.2 M1 Max): 4개 클래스 모두 존재(`CGVirtualDisplay` 21 메서드 등). descriptor `maxPixels = 논리×scale×2`, `sizeInMillimeters 520×325`, vendor `0x4C50`/product `0x4C43`, serial non-zero; **모드는 논리 크기 + `hiDPI` 플래그**(미리 곱하면 2중 스케일); 생성자 반환 ≠ 등록 보장 → `CGDisplayIsActive`/모드 폴링(2s); 배치는 `CGBegin/CGConfigureDisplayOrigin/CGComplete(.forSession)` + bounds 폴링 + 메인 디스플레이 불변 확인.
- 실패 분류: `UNAVAILABLE`(GUI 세션 없음) / `NOACTIVE`(활성 디스플레이 0 — **생성 불가 전제**, clamshell·headless 전원 불가는 업계 공통) / `FAILED displayID=0`.
- 피해야 할 것: BetterDisplay CLI(aspect=pixels 사고, discard 풀폭발, headless abort(134)), 고정 sleep, 앱 Resources에 shim 번들(78MB, codesign 실패), SSH/자동화 셸에서 생성 시도(GUI 세션 없음).

### 1.4 외부 생태계 상태 (2026-09 웹 리서치)

- CGVirtualDisplay는 macOS 26에서 **살아있고 오히려 확장 중**(26.4 덤프에 `rotation`/`isReference`/`transferFunction`/`displayInfo` 추가). go-macos/virtualdisplay가 macOS 26.6.2 M4 Max에서 동작 실측. Chromium이 바이너리에서 생성한 인터페이스를 in-tree 유지. BetterDisplay도 동일 API 사용.
- 핵심 동작론: **객체를 해제하면 디스플레이가 사라진다**(파괴 API 별도 없음). **프로세스 죽으면 WindowServer가 회수**(SIGKILL 포함 — 옛 설계의 stale 디스플레이 문제가 OS에서 무료 해결). 제거는 비동기(최대 ~2s 폴링).
- **최대 함정: 모드 변경.** 공개 모드 API(`CGDisplaySetDisplayMode`/`CGBeginDisplayConfiguration` 경로)로 모드를 바꾸면 **제거 불가능한 좀비 디스플레이**가 될 수 있음(go 실측, Chromium 제거 flaky 보고). 해법 = 생성 시 최종 모드로 만들고 모드 변경 금지, 리사이즈가 필요하면 `applySettings:`(비공개, OpenDisplay 방식) 또는 destroy+recreate.
- macOS 14+ 요구: vendorID non-zero, 동시 디스플레이 간 serial 고유. **identity(vendor/product/serial) 기준으로 macOS가 배치·모드를 기억하고 생성 수 초 후 비동기 복원** → 고정 identity는 "같은 모니터로 인식"에 유리(제안 문서의 display identity 목표를 API가 지원).
- 기타: mirror-set 함정(생성 직후 "전체 화면 미러"로 분류될 수 있음 — 세션 스코프로 해제), ICC 프로필이 identity당 1개 `/Library/ColorSync/Profiles/Displays/`에 영구 생성(고정 identity로 축적 방지), 60Hz 상한, ScreenCaptureKit 캡처 정상 동작.

---

## 2. 제안 설계 문서와의 대조

| 제안 문서 항목 | 현재 구조와의 관계 | 판정 |
|---|---|---|
| `VirtualDisplayProvider` trait + lease | 옛 provider.rs(1,345행)가 동일 개념이었으나 제거됨. v1은 단일 엔진·단일 디스플레이라 **trait 대신 얇은 모듈**이 적합(추상화 선재입 안 함) | 축소 채택 |
| "별도 pipeline 금지, source 취급" | §1.1 — 무수정 충족 | 그대로 |
| capability detection → degrade | 기존 패턴 존재: 옵션 심볼 프로브 + trait default(`ffi.rs:715-727`, `backend.rs:72-77`의 `screen_permission` | 동일 패턴 채택 |
| viewer capability 협상 | 뷰어는 오늘날 자기 디스플레이 정보를 **아무것도 안 보냄**(옛 `viewerDisplay`가 유일). v1은 메트릭 보고+기본 모드 도출까지만 최소 반영, 협상(생성 요청 등)은 후속 | 최소 채택 |
| HiDPI 논리/backing 분리 | 모드=논리+hiDPI 플래그로 해결(§3) | 채택 |
| update_mode (회전 등) | **모드 변경 = 좀비 리스크** → v1 금지, destroy+recreate(동일 identity)로 대체 | 수정 채택 |
| window resize ≠ display resolution | 이미 성립: 뷰어가 startStream/reconfigureStream으로 세션 크기만 바꾸고 가상 디스플레이 모드는 불변 | 무수정 |
| disconnect → destroy / grace | **세션 스코프 소유**(§6 결정 1): 라이브 뷰어 세션이 0이 되는 전환(네트워크 단절·뷰어 강제 종료 포함)에 destroy한다. grace 타이머는 두지 않으며, 앱 종료 시에도 상한 2초 동안 동기 해제한다(44e84ff). | 정책 확정·구현 |
| Host crash cleanup | WindowServer가 프로세스 사망 시 회수(외부 실측) — 유효성만 실물 확인 | 무료 해결 |
| 보안: 호스트 승인 | **호스트 UI가 생성 주체**인 구조로 충족(제안 문서의 대안 2). 뷰어 요청 경로·새 명령 없음 = 공격면 0 | 채택 |
| USB 우선 | startStream이 이미 udp/tcp/usb/adb 순 시도(control.rs:2428 전후) | 무수정 |
| 관찰성 | createMs/modeSetMs는 FFI 결과로 호스트 로그·상태에 노출, first-frame 타이밍은 기존 `first_send_ms` 재사용 | 최소 추가 |
| 다중 가상 디스플레이 | v1 1개, 단 identity(시리얼 슬롯) 기반 구조로 확장 막지 않음 | 채택 |

---

## 3. 모드/HiDPI 정책

기본 모드는 **대상 클라이언트 기기의 메트릭에서 도출**하고(2026-09-18 사용자 피드백 반영), 프리셋은 폴백·수동 선택용:

| 프리셋 | 논리 | backing (hiDPI=1) | 비고 |
|---|---|---|---|
| **1280×800 @2x 60Hz (기본)** | 1280×800 | 2560×1600 | M1 baseline, Retina 텍스트 + 부하 균형 |
| 1440×900 @2x 60Hz | 1440×900 | 2880×1800 | |
| 1600×1000 @2x 60Hz | 1600×1000 | 3200×2000 | |
| 2560×1600 네이티브 | 2560×1600 | 2560×1600 (hiDPI=0) | 텍스트 소형, 부하 최소 |

기본 모드 도출(생성 UI에서 대상 기기 지정 시):
1. 뷰어가 startStream에 `viewerDisplay`(패널 물리 픽셀)를 선택 필드로 실어 보내면(§4.5) 호스트가 기기(owner)별 최근값 캐시 → 생성 UI에서 대상 기기를 고르면 그 값으로 도출. 어떤 소스든 스트리밍하는 순간 학습되므로(내장 디스플레이 공유 중에도), 한 번이라도 쓴 기기는 도출 가능.
2. 도출 규칙은 옛 `display_matching.rs`(git `f1a846e^`)의 순수 수학 부활: 논리 = even(physical/2), @2x(backing = physical), **종횡비는 패널 그대로**(letterbox 제거 — 16:9 태블릿이 16:10 가상 디스플레이에 검은 줄 생기는 문제 해소), 논리 최소 1280×720 미만이면 scale-1 폴백, 장변 정규화(가로 기준), 상한 클램프(논리 ≤ 1600×1000). 뷰어 값을 그대로 신뢰하지 않고 호스트가 클램프·검증(제안 문서 요구).
3. 메트릭 없는 기기(최초 1회, XR처럼 패널 개념이 흐린 기기 — 뷰어가 전송 안 하는 선택지 포함) → 프리셋 폴백.
4. 주사율은 클라이언트와 무관하게 60Hz 고정(private API 상한 — 클라이언트 입력이 반영되는 건 해상도·종횡비만).

규칙:
- 모드는 생성 시 1개만 선언(정확히 요청 모드 — DeskPad #48 교훈). `maxPixels = backing` 정확히(회전 미지원 v1이므로 ×2 여유 불필요).
- descriptor: `name "Leftcar Display"`, `vendorID 0x4C50`, `productID 0x4C43`, `serialNum` 슬롯별 고정값(예: 0x4C430001), `sizeInMillimeters 520×325`, `dispatchQueue` 전용 직렬 큐, `terminationHandler nil`.
- 생성 후 검증 폴링: `CGDisplayIsActive` 2s → `CGDisplayCopyDisplayMode`가 논리 크기 && `pixelWidth == 논리×scale` 2s. `@2x`가 바로 안 나오는 경우(부팅 1x 문제 보고 있음)에만 별도 실험 항목(E-3)로 처리 — 공개 모드 API 사용 금지.
- 리사이즈 = destroy → 동일 identity로 recreate(macOS가 배치를 identity로 복원). `applySettings:` 라이브 리사이즈는 후속 검증 후 옵션.

---

## 4. 아키텍처 (최소 변경)

```text
[Host UI] 확장 디스플레이 카드 (App.tsx)
     │ Tauri 명령 2개: virtual_display_create / virtual_display_remove (+status는 get_status 확장)
     ▼
[Rust] src/virtual_display.rs (신규, 얇음)
     │ 옵션 FFI 심볼 프로브 (leftcar_vdisp_*_v1) — 없으면 기능 OFF
     ▼
[Shim] native/macos-capture-shim/Sources/VirtualDisplay/ (신규, 격리)
     │ CGVirtualDisplay 런타임 브리지 (NSClassFromString + objc_msgSend)
     │ 객체 강한 참조 보유 = 디스플레이 수명 (프로세스 = 호스트 앱)
     ▼
CGDirectDisplayID → 기존 열거/승인/캡처/입력/복구 전체 무수정
```

### 4.1 네이티브 모듈 (격리 요구사항 반영)

- 위치: 기존 capture shim **내부의 독립 서브디렉터리** `Sources/VirtualDisplay/`. 제안 문서의 `native/macos-virtual-display` 별도 dylib은 채택하지 않음 — 로더·서명·설치 경로가 늘어나는데 v1 격리 이득이 없음. 심볼이 옵션이므로 같은 dylib 안에서 완전히 격리된다.
- 구현: ObjC 브리지 파일(`CGVirtualDisplayBridge.h/.m`, CGVD.h 유산 시그니처) + clang 스텝을 `tools/build-macos-capture-shim.zsh`에 소폭 추가. 링크타임 심볼 의존 0(전부 `NSClassFromString` + `objc_msgSend`) → API 부재 시 컴파일·로드 모두 무관. 대안(빌드 스크립트 무수정)으로 Swift `@convention(c)` IMP 캐스팅이 가능하나 가독성이 나빠 2순위.
- 프로브(tier): (0) 4개 클래스 + 핵심 셀렉터 존재(`class_getInstanceMethod`) → 없으면 `supported=false`, 크래시 불가능 구조. (1) 활성 디스플레이 ≥1 + GUI 세션(`CGSessionCopyCurrentDictionary`). (2) 실제 create는 첫 사용 시.
- 심볼 세트(v1): `leftcar_vdisp_probe_v1() -> JSON{supported, reason}`, `leftcar_vdisp_create_v1(name, logicalW, logicalH, scale, serial) -> JSON{displayId, uuid, sourceId, createMs, modeVerified}`, `leftcar_vdisp_place_v1(displayId, x, y) -> i32`, `leftcar_vdisp_status_v1() -> JSON`, `leftcar_vdisp_destroy_v1(displayId, serial) -> i32`(제거 폴링 ~2s 포함).
- 배치 기본값: 주 디스플레이 오른쪽(`CGConfigureDisplayOrigin` 세션 스코프 + bounds 폴링 + 메인 불변 확인 — 옛 PLACE 레시피). 이후 사용자가 시스템 설정에서 정한 arrangement는 존중(재배치 안 함).

### 4.2 Rust 모듈

- `src-tauri/src/virtual_display.rs` 신규: 심볼 프로브(Option), 생성/제거/상태, **영속 레코드 없음** — 상태는 프로브+`leftcar_vdisp_status_v1`로 항상 실시간 조회(프로세스 수명 = 디스플레이 수명이므로 서버 재시작 시 자기 수순). 옛 display_management.rs의 persisted registry·reeattach는 불필요(프로세스 밖 생존이 아예 없음).
- `CaptureBackend` trait 확장 없음 — 가상 디스플레이는 캡처 백엔드가 아니라 그냥 display. `get_status`에 `virtual_display: Option<VirtualDisplayStatusPublic>` 추가.
- Tauri 명령 2개 추가: `virtual_display_create(mode_preset)`, `virtual_display_remove()`. 창 종료·앱 종료 시 정리는 객체 drop이 수행(모든 경로 보장).

### 4.3 카탈로그 이름 오버라이드 (유일한 shim 캡처 측 변경)

- 등록 테이블: `leftcar_vdisp_register_catalog_name_v1(uuid, name)` — `coreGraphicsCatalogJSON()`(`CaptureBackend.swift:203-223`)가 UUID 일치 시 이름 사용. 옛 managed-mode-table(`leftcar_capture_register_managed_display_mode_v1`)과 같은 패턴의 최소 재현.
- `nativePixelSize`가 가상 디스플레이 모드를 못 보는 경우(CGDisplayCopyDisplayMode nil — 옛 코드가 겪은 문제) 대비: 같은 테이블에 (논리, backing)을 등록해 폴백. Phase 1 실험(E-2)에서 필요성 판정.

### 4.4 Host UI

- 대시보드에 "확장 디스플레이" 카드(IdleStudioView/App.tsx:1074-1099 인접): 지원 여부 표시, 모드 프리셋 선택, 만들기/제거, 활성 시 source 이름·스트리밍 상태.
- 미지원 사유 배너: `SystemAlertBanners`(App.tsx:856-955) 패턴 재사용 — 사유별 문구(GUI 세션 없음/활성 디스플레이 없음/API 미지원 macOS).
- grants 연동: 생성 완료 시 페어드 기기 목록에서 이 source를 허용할 기기 선택 → `set_source_grants` 호출. ⚠️ 진행 중인 pairing 리팩터(승인 시 전체 자동 허용, grant editor 삭제)와 충돌하지 않게 **병합 리베이스 직전에 구현 착수 금지 — 조율 후 착수**.
- i18n: `packages/ui-tokens/src/i18n.ts` host 섹션 ko/en. 뷰어 측 변경 0(카탈로그에 새 디스플레이로 나타남).

### 4.5 계약(control-contract): 선택 필드 1개 부활

- 기본 모드 도출을 위해 옛 형태 그대로 `StartStreamInput.viewerDisplay: Option<ViewerDisplayMetricsMsg{physical_width, physical_height, density_dpi}>` 부활(serde 옵셔널 — 양방향 역호환, `f1a846e^`에서 검증된 형태). plain serde 타입이라 rustra 코드젠·계약 해시 무영향, 뷰어 손작성 TS 미러(`apps/viewer-expo/src/control.ts`)만 1필드 추가.
- 생성 명령 등 신규 wire 커맨드는 여전히 없음(호스트 주도 유지). 뷰어 요청 기반 생성이 필요해지면 그때 `CreateVirtualDisplayInput` 계열 추가.
- 뷰어 전송 시점: 모든 startStream에 첨부(어떤 소스든). 호스트는 기기별 최근값만 캐시하고 다른 동작을 하지 않음 — 옛 `prepare_viewer_display`와 달리 자동 리사이즈·재사용 판단은 하지 않음.

### 4.6 입력/커서/오디오

- 입력: 무수정(§1.1). 커서: 기본 임베디드 유지(중복 커서 방지 — 제안 문서 요구와 일치), 클라이언트 커서 토글(LCDON/LCDOFF)도 기존 그대로 동작. 오디오: 기존 소스별 정책 그대로.

---

## 5. 라이프사이클

| 시나리오 | v1 동작 |
|---|---|
| 뷰어 연결 종료 | 마지막 라이브 뷰어 세션이 제거되어 세션 수가 0이 되는 전환이면 디스플레이를 destroy한다(§6 결정 1). 네트워크 단절·뷰어 강제 종료 같은 비자발적 단절도 미디어 타임아웃/만료 또는 후속 연결의 stale-session 정리를 거쳐 같은 규칙을 따른다. |
| 뷰어 재연결 | 제거가 완료됐으면 고정 identity의 디스플레이를 다시 생성한다. 제거 직후에는 최대 ~35s `removal pending` 윈도우가 생길 수 있으며, 기존 뷰어의 pending 가드와 상태 폴링이 재열기를 보류한다. |
| 호스트가 제거 클릭 | destroy → 제거 폴링 → 캡처 중이면 `markStopped`→LCT1→리퍼가 기존 경로로 정리. **순서 보장: 캡처 종료를 기다리지 않고 destroy → 기존 소실 처리 경로가 후행**(옛 설계의 수동 순서 조율 불필요) |
| 호스트 크래시/강제종료 | WindowServer가 프로세스 사망 시 회수(외부 실측) → E-1 실물 확인. 앱 재시작 시 status 조회로 정합 |
| sleep/wake | 캡처는 기존 와치독(2.5s 무콜백 제자리 재시작)과 `didStopWithError` 경로가 처리. 디스플레이 객체는 프로세스 생존. wake 후 mirror/모드 비동기 복원은 E-4 확인 |
| 생성 실패 | 오류 표시 후 캡처 미시작(제안 문서 요구). 실패 분류 UNAVAILABLE/NOACTIVE/FAILED를 그대로 UI 문구화 |

---

## 6. 사용자 확인이 필요한 결정

1. **소유 정책(확정: 세션 스코프 소유)** — 정책 요약은 **세션 스코프 소유 + 라이브 뷰어 세션 0 전환 시 destroy + 앱 종료 시 상한 2초 동기 해제(44e84ff)**다. 명시적 생성은 호스트가 수행하지만, 한 개 이상이던 라이브 뷰어 세션이 0이 되는 teardown 전환에서 화면을 해제한다. 이 규칙은 정상 `stopStream`뿐 아니라 네트워크 단절·뷰어 강제 종료 같은 비자발적 단절에도 적용하며 별도 grace 타이머를 두지 않는다. 재열기 시 제거가 아직 반영 중이면 최대 ~35s의 `removal pending` 윈도우가 생길 수 있지만, 기존 뷰어의 pending 가드와 상태 폴링이 이를 흡수한다. 구현 근거는 `control.rs`의 `release_virtual_display_if_idle`과 네 호출 지점(`teardown_targets`, 만료 세션 스윕의 `session_expired`, `stop_sessions_for_viewer`의 `viewer_superseded`, `stopStream`의 `stop_stream`)이며, `lib.rs`의 `RunEvent::Exit`는 `remove_blocking_within(Duration::from_secs(2))`로 종료 시 해제를 최대 2초 동기 대기한다.
2. **기본 모드(권장: 클라이언트 기기 메트릭 도출 + 프리셋 폴백)** — 2026-09-18 피드백 반영. 대상 기기 패널의 종횡비·해상도를 그대로 반영(physical/2 @2x, 호스트 클램프)해 letterbox 없음. 메트릭 부재 시 폴백 1280×800 @2x 60Hz. 주사율은 60Hz 고정(API 상한). 현재 주 기기(TB710FU 2560×1600)에서는 도출 결과가 폴백과 동일(1280×800@2x).
3. **생성 주체(권장: 호스트 전용)** — 뷰어 요청+승인 경로는 계약 변경·승인 UI가 필요해 후속으로 분리.
4. **grants UX 병합 시점** — 진행 중인 pairing/grants 리팩터(타 세션)와 충돌 방지를 위해 확장 디스플레이 UI 착수를 그 병합 이후로 배치.

## 7. 검증 계획 (물리 게이트 — 이번에는 반드시)

실험 항목(실기기, GUI 터미널에서 직접 — SSH 불가 제약 준수):
- E-1 크래시 회수: 생성 → 호스트 kill -9 → 디스플레이 소실 확인.
- E-2 카탈로그/모드 가시성: `nativePixelSize`가 backing 2560×1600을 반환하는지(아니면 §4.3 테이블 활성화).
- E-3 @2x 직접 부팅: hiDPI=1 선언만으로 pixel==논리×2로 올라오는지(아니면 E-3b 대안 필요 — 공개 모드 API는 금지).
- E-4 sleep/wake 후 존속·미러 없음·캡처 복구.
- E-5 반복 생성/제거 5회 축적 — ICC 프로필(/Library/ColorSync/Profiles/Displays/) 1개로 수렴 확인, 좀비 디스플레이 없음.
- E-6 스트리밍 회귀: 물리 디스플레이 스트리밍이 생성/제거 전후 동일(stream-stats 분포 비교).
- E-7 실기기 종단: 태블릿에서 생성 디스플레이 스트리밍 + 터치/키보드 좌표 정확성 + XR 창 종횡비.
- E-8 PLACE(배치 설정) 후에도 제거가 되는지(모드-변경-좀비 함정의 배치 버전).

레거시 체크리스트(`tablet-display-physical-validation.md`)의 덮개 닫기 항목은 활성 라이브 뷰어 세션 동안의 화면 생존과, 세션 0 전환 뒤 해제를 함께 확인한다. 활성 디스플레이 0 환경에서는 생성 전제도 별도로 확인한다.

각 단계 게이트: shim 단위 테스트(기존 단일 파일 테스트 exe 패턴) + `cargo test`(src-tauri는 루트 워크스페이스 외부 — 개별 실행) + React UI 단계는 react-doctor 100/100·typecheck·테스트 + 전 단계에서 기존 스트리밍 회귀 없음.

## 8. 단계별 계획

- **Phase 1 — 네이티브**: `Sources/VirtualDisplay/` ObjC 브리지 + 프로브/create/destroy/place + 이름·모드 등록 테이블 + 빌드 스크립트 clang 스텝 + 단일 파일 테스트 exe. 게이트: E-1~E-3, E-5 (수동 프로브로 실물 확인).
- **Phase 2 — Rust+UI**: virtual_display.rs + Tauri 명령 2개 + 카드/배너 + grants 연동 + i18n + `viewerDisplay` 선택 필드 부활(뷰어 전송·호스트 기기별 캐시) + 옛 display_matching 순수 수학 부활(기본 모드 도출, 단위 테스트). 게이트: cargo test, react-doctor 100, E-8.
- **Phase 3 — 종단**: 실기기 E-4/E-6/E-7, stream-stats 회귀 비교.
- **Phase 4(선택 후속)**: 회전·세로 방향 지원, `applySettings:` 라이브 리사이즈, 복수 디스플레이(시리얼 슬롯 확장), 뷰어 요청+승인 경로, 메트릭 변화 감지 재도출.

## 9. 하지 않을 것 (v1)

BetterDisplay 연동(영구), 라이브 모드 변경, 회전, 4K/120Hz, 다중 가상 디스플레이, 세션 종료 grace 타이머, 뷰어 주도 생성, rustra 계약 변경, 새 인코더/전송/디코더, HDR/4:4:4, 색정보 필드(`redPrimary` 등 미검증 surface).

## 10. 리스크

| 리스크 | 등급 | 완화 |
|---|---|---|
| private API가 macOS 포인트 릴리즈에서 파손 | 중(생태계상 단기 가능성 낮음) | 런타임 프로브 → 기능 OFF 배너, 기존 화면 공유 무영향(격리 요구 충족) |
| 모드/배치 설정이 제거 불가 좀비 유발 | 중 | v1은 모드 변경 없음. 배치는 세션 스코프 + E-8로 검증. 최악 시 앱 종료로 회수 |
| @2x가 바로 안 붙는 경우 | 중 | E-3에서 확인, 실패 시 프리셋을 네이티브 모드로 자동 대체 + 후속 검증 |
| 뷰어 메트릭 부정확(XR 창 크기 등) | 낮음 | 호스트 클램프·상한 + 메트릭 미전송 시 프리셋 폴백 |
| 진행 중 pairing/grants 리팩터와 충돌 | 중 | UI 착수를 병합 후로 배치(결정 4) |
| 생성 전제(활성 디스플레이 ≥1, GUI 세션) | 낮음(업계 공통) | 사유별 배너 문구. 덮개 닫힘/헤드리스에서 시도 시 명확한 안내 |
| ICC 프로필 누적 | 낮음 | 고정 identity(시리얼 슬롯)로 1개 수렴 |

## 11. 구현 결과 (2026-09-18 실측)

구현: ObjC 브리지(`native/macos-capture-shim/Sources/VirtualDisplay/`) + shim 심볼 6종(`leftcar_vdisp_*`), Rust 매니저(`src-tauri/src/virtual_display.rs`) + Tauri 명령 3건, 계약 `viewerDisplay` 부활(해시 불변 확인), 뷰어 네이티브 `getDisplayMetrics`(Kotlin) + startStream 첨부, 호스트 UI 카드(확장 디스플레이, ko/en).

게이트: src-tauri 250 · Kotlin 142 · 루트 vitest 748 · 계약 34+4 전부 통과, react-doctor 100/100, typecheck 클린. 호스트 재빌드·DR 보존 설치 완료(TCC 유지).

물리 검증 결과(이 Mac, macOS 26.6.2):
- **E-1 통과** — 소유 프로세스 kill -9 시 즉시 회수. 실운영에서도 재현: 호스트 재시작 시 가상 디스플레이 소실(설계대로).
- **E-2 통과** — 카탈로그 자동 편입, 고정 UUID(`macos:display:5127cb43-…`)가 세션 간 불변.
- **E-3 통과(우회 필요)** — 선언만으로는 최대 네이티브 모드(2560×1600@1x)로 부팅. **`applySettings:` 재적용 1회로 @2x 확정**(bounds 기준 2-5s 수렴, 30초 관찰 안정). 생성 프로세스의 `CGDisplayCopyDisplayMode`는 부팅 모드에 갇혀 갱신되지 않음(외부 프로세스는 정상) — 검증은 bounds로 함.
- **E-5 통과** — 3-5회 생성/제거 반복에서 좀비 없음. ICC는 고정 identity로 정확히 1개 재사용. 단 **프로세스 생존 중 제거는 ~30초 비동기**(빈 세션 재구성 트릭도 무효) — UI는 "제거 중" 상태로 처리.
- **E-8 부분 통과** — 실제 UI 버튼 → Tauri → 브리지 생성 성공(감사 로그 `virtual_display_created`, modeVerified:true, 프리셋 셀렉트 동작 포함). 스트리밍 회귀 동시 확인(기존 화면 s1 2560×1440@60 enc=60/rend=58 running). 뷰어가 가상 디스플레이를 직접 스트리밍하는 E-7과 입력 매핑은 신규 APK + 승인 갱신 후 남는 과제.

운영 참고: 스트리밍 중인 기기는 생성 시 승인 갱신에서 제외된다(갱신이 라이브 세션을 끊음) — 해당 기기는 다음 생성 시점(연결 없을 때) 또는 페어링 UI에서 승인한다.
