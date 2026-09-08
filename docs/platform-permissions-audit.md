# 플랫폼별 권한 감사 및 다음 작업 정리

작성일: 2026-09-07.
목적: 뷰어 주도 화면 크기 기능(feat/viewer-driven-display-sizing) 완료 후, 플랫폼별
권한/entitlement 현황을 정리하고 남은 검증·후속 작업의 우선순위를 확정한다.

## 플랫폼별 권한 현황

### macOS 호스트 — 구현됨

- `apps/host-desktop/src-tauri/Info.plist`: `NSScreenCaptureUsageDescription` 유일.
  `tauri.macos.conf.json`의 `bundle.macOS.infoPlist`로 반영.
- TCC 화면 녹화 권한: `native/macos-capture-shim/Sources/CaptureShim.swift`의
  `CGPreflightScreenCaptureAccess()` 사전 점검만 사용. `CGRequestScreenCaptureAccess`
  (권한 요청 프롬프트)는 의도적으로 호출하지 않음 — 새 TCC 신원의 릴리스 번들이 사용자
  입력 대기로 멈추는 문제 때문. 권한이 없으면 카탈로그/시작이 에러로 실패하고 시스템
  설정에서 수동 허용해야 함. 실측에서도 "display catalog warmup deferred" 경고로 확인됨.
- `persistent-content-capture` entitlement: `PersistentCapture.entitlements.example`로만
  존재, 미활성. 런타임에 `SecTaskCopyValueForEntitlement`로 실제 여부를 확인해
  persistent SCK 경로를 게이트. Apple 승인 전까지 ad-hoc.
- hardened runtime / 활성 entitlements: `2026-08-25-v0.2-hardening.md` 계획에 따라
  의도적으로 연기됨.

빠진 것: 인앱 TCC 안내 흐름(거부 상태에서 사용자에게 "시스템 설정 → 화면 녹화" 안내
UI).

### Android 뷰어 — 구현됨 (Expo 앱이 주력)

- `apps/viewer-expo/android/app/src/main/AndroidManifest.xml`: `INTERNET`,
  `ACCESS_NETWORK_STATE`, `WAKE_LOCK`, `ACCESS_WIFI_STATE`,
  `CHANGE_WIFI_MULTICAST_STATE`(mDNS), `SYSTEM_ALERT_WINDOW`, `VIBRATE`,
  maxSdk 32 스코프 저장소. `usesCleartextTraffic="true"`(LAN 스트리밍).
- USB: MainActivity에 `USB_ACCESSORY_ATTACHED` 인텐트 필터 + accessory_filter
  (AOAP 호스트 감지).
- `SYSTEM_ALERT_WINDOW`은 선언만 있고 `canDrawOverlays` 요청 코드가 없음 —
  사실상 잔여 선언. 제거하거나 실제 오버레이 기능(FPS 표시 등)에 연결할 것.
- 포그라운드 서비스 없음: 시청 전용 앱이라 당장 문제없으나, 화면 꺼짐 상태 장시간
  수신·백그라운드 지속 요구가 생기면 `FOREGROUND_SERVICE_MEDIA_PLAYBACK` 계열 검토.
- `expo-camera` 권한 문자열은 QR 페어링용.
- 레거시 `apps/viewer-android`는 최소 권한만 사용(유지보수 대상 아님).

빠진 것: 런타임 권한 요청 코드(필요한 것 없음), 사용하지 않는 overlay 권한 정리.

### iOS 뷰어 — 미구현, 구체적 계획 없음

- `ios/` 디렉터리, Podfile, entitlements, ReplayKit/Broadcast Upload Extension 코드
  전부 없음. `app.config.ts`에 iOS 섹션 없음.
- iOS에서 화면 "수신" 뷰어는 ReplayKit 없이 가능(단순 디코딩 표시). 다만 백그라운드
  재생, Picture-in-Picture, 네트워크 멀티캐스트 권한 등 별도 검토 필요.
- 결론: 권한 걱정보다 "플랫폼 지원 여부" 자체가 결정 사항. 로드맵에 명시적으로
  포함/제외할 것.

### Windows 호스트 — 코드 존재, 실기 미검증

- `apps/host-desktop/src-tauri/src/windows_backend/`: Windows Graphics Capture
  (`CreateForMonitor` + frame pool), MF 하드웨어 H.264, `SendInput`.
- **WGC 동의 처리 없음**: `GraphicsCaptureAccess.RequestAccessAsync` 호출이
  `capture.rs`에 없음. Windows 10 1903+ 에서 모니터 캡처는 사용자 동의(노란 테두리,
  설정의 "그래픽 캡처" 허용)에 묶이므로 실기 검증 시 필수 확인 항목.
- `SendInput`/UIPI 제약: 상승된 권한 앱 제어 불가 — 문서화됨.
- 설치본 미서명.
- 물리 Windows 빌드/실행 자체가 대기 상태(로드맵 H39).

## 권한 관련 결론

- 지금 당장 권한 때문에 막히는 플랫폼은 없다. macOS TCC는 "수동 허용" 설계이고,
  Android는 필요 최소권한, iOS는 미지원, Windows는 실기 검증 전 단계.
- 다만 사용자 질문의 지적대로 플랫폼별 "받을 수 있는 것" 차이는 실재한다:
  - macOS: persistent-content-capture(Apple 승인 필요) 유무가 장시간 캡처 안정성을 가름.
  - Windows: WGC 동의 + UIPI 제약 → macOS와 동등한 제어 범위를 못 얻을 수 있음.
  - Android: 백그라운드 지속성 요구 시 서비스 권한 추가 필요.
  - iOS: 지원 여부 결정이 선행.
- 권장: 권한 항목을 플랫폼 지원 매트릭스(로드맵)에 한 테이블로 반영하고,
  Windows 실기 검증 체크리스트에 WGC 동의/노란 테두리/UIPI 항목을 명시할 것.

## 다음 작업 우선순위

viewer-display-sizing-validation.md의 미검증 항목 + 위 권한 항목을 합쳐 정리:

1. **Galaxy XR 실기 검증** — 비율 프리셋 실시간 전환(재시작 없음), Mac 가상 해상도
   불변 확인, config-change/process-death 후 비율 복원, 9:16 세로와 home-space
   bounds 충돌. (기능 완성도의 마지막 미검증 축)
2. **cgvd-shim RESIZE GUI 세션 실측** — 모드 전환·지연, maxPixels 거부→FAILED,
   scale 1↔2 왕복, RESIZE 직후 PLACE.
3. **macOS TCC 안내 UX** — 권한 거부/부재 상태를
   호스트 UI에 명확히 표시(현재는 로그 경고뿐).
4. **Android 권한 정리** — 미사용 `SYSTEM_ALERT_WINDOW` 제거(또는 실기능 연결),
   장시간 수신 시나리오에 대한 포그라운드 서비스 필요성 판단.
5. **Windows 실기 검증 준비** — WGC 동의 흐름(`RequestAccessAsync` 추가 여부 포함),
   미서명 설치본, UIPI 제약 체크리스트. (H39)
6. **iOS 지원 결정** — 지원하면 Expo iOS 빌드 + 백그라운드/PiP 권한 설계부터.

연관 문서: docs/viewer-display-sizing-validation.md (미검증 상세),
docs/08-implementation-roadmap.md (H39), docs/windows-remote-host.md,
docs/macos-persistent-capture.md.
