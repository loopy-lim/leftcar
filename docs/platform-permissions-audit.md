# 플랫폼 권한과 남은 검증

기준: 2026-09-13 completion-followup 소스. 최초 2026-09-07 조사의 가상 디스플레이 복구 제안과 오래된 권한 목록을 현재 구현에 맞게 교체했다. 현재 후보의 최종 빌드/실기기 상태는 [완료와 지원 기준](completion-and-support.md)을 따른다.

## macOS Host

- 캡처는 display-only다. 화면 녹화 TCC 상태를 shim의 `CGPreflightScreenCaptureAccess`로 확인하고 Host 대시보드에서 설정 열기/재시작을 안내한다. 권한 없이 캡처에 성공한 것으로 처리하지 않는다.
- 원격 입력은 Host 세션별 기본 OFF이며, 손쉬운 사용 권한과 명시적 세션 허용이 모두 필요하다. OS 권한을 얻어도 source grant나 입력 토글을 대체하지 않는다.
- `persistent-content-capture`는 예제 entitlement와 런타임 게이트만 존재한다. Apple 승인·활성 entitlement·장시간 안정성은 각각 확인해야 한다. entitlement가 없어도 내부 후보를 빌드할 수 있지만 해당 권한을 가진 배포본으로 설명하지 않는다.
- `tauri.macos.conf.json`의 개발용 signingIdentity와 내부 build의 ad-hoc 서명, Developer ID/notarization은 다른 상태다. 현재 내부 빌드 도구의 실제 `codesign` 검사와 manifest를 따른다. 설정 파일의 identity만으로 서명을 확인했다고 하지 않는다.
- 사용자 TCC 허용, 권한 철회, Host/Viewer 종료 후 캡처 해제, 디스플레이 sleep/wake는 실기기 체크에 남는다. 가상 디스플레이 관리 복원은 현재 완료 범위가 아니다.

## Android Viewer

주력은 `apps/viewer-expo`다. 네이티브 main manifest는 네트워크·Wi-Fi/mDNS·wake lock·진동, USB accessory 진입을 선언한다. QR용 CAMERA 등 의존성이 병합하는 권한은 최종 manifest/APK에서 함께 확인한다.

배포용 `src/release/AndroidManifest.xml`은 `READ_EXTERNAL_STORAGE`, `WRITE_EXTERNAL_STORAGE`, `SYSTEM_ALERT_WINDOW`를 merger remove로 제거한다. 파일 선택은 `expo-document-picker`와 앱 전용 파일 경로를 이용하므로 전체 외부 저장소 권한을 요구하지 않는다. debug에는 Expo/RN 개발 기능에 필요한 기존 선언을 유지한다. 오버레이 기능을 새로 만들지는 않았다.

`usesCleartextTraffic="true"`는 플랫폼의 cleartext 허용 설정이다. 이것만으로 Leftcar 제어/미디어가 평문이라고 판단하지 않는다. LAN 제어는 애플리케이션의 인증 암호 채널, 미디어는 세션 AEAD로 보호한다. 루프백 진단과 물리 AOAP 제어의 예외는 [보안 문서](07-security-privacy.md)에 명시돼 있다.

백업은 앱 전체를 끄지 않고 `SecureStore` 및 레거시 `ReactNativePreferences.xml` shared preferences를 제외한다. full backup, cloud backup, device transfer의 세 경로를 모두 설정한다. 자체 XML을 사용하므로 `app.config.ts`의 `expo-secure-store.configureAndroidBackup`은 false다. Expo는 복원 뒤 키를 잃어 해독할 수 없는 SecureStore 항목을 이렇게 제외하도록 안내한다. [Expo 공식 문서](https://docs.expo.dev/versions/latest/sdk/securestore/#android-auto-backup), [Android 백업 규칙](https://developer.android.com/identity/data/autobackup)

2026-09-13 로컬 Gradle의 `processReleaseMainManifest`/`processDebugMainManifest`가 성공했고, release에서 위 세 권한 부재와 양쪽 variant의 소유 XML 참조를 검사했다. 이후 최종 내부 APK의 manifest/resource table/compiled XML에서도 세 권한 부재와 세 백업 경로의 제외 규칙을 확인했다. 정확한 파일과 해시는 [후속 검증 기록](2026-09-13-completion-followup-validation.md)을 따른다. 실제 backup/restore·앱 업데이트는 별도 미실행이다.

현재 StreamActivity는 recent task에 표시되며 `FLAG_SECURE`를 쓰지 않는다. screenshot/preview 차단을 보장하지 않는다. 해당 flag는 screenshot과 비보안 display 표시를 제한하므로, Galaxy XR Home Space/Surface 호환성 확인 없이 켜지 않는다. 태블릿과 XR에서 스트림 표시, task 전환, 복귀, screenshot/preview를 따로 검증한 후 결정한다. [Android secure activity 안내](https://developer.android.com/security/fraud-prevention/activities)

백그라운드·화면 OFF 지속 수신은 현재 지원 수락 범위가 아니다. foreground service를 선언해 놓았다는 식으로 장시간 수신을 추정하지 않는다. `apps/viewer-android`는 레거시 경로이며 현재 후보 APK의 근거로 사용하지 않는다.

## Windows Host

WGC `CreateForMonitor`와 Media Foundation H.264, `SendInput` 구현이 있다. PR #5의 Windows 빌드/NSIS CI 성공은 패키징 증거이며 실제 디스플레이·GPU·입력 결과가 아니다.

이 구현은 Win32 monitor interop 경로다. 일반 WGC picker 문서와 동일하게 `RequestAccessAsync` 호출을 반드시 추가해야 한다고 단정하지 않는다. 사용 OS에서 `IsSupported`, 캡처 시작/실패, 보호 콘텐츠, 노란 테두리, 종료 뒤 자원 해제를 확인한다. `CreateForMonitor`의 공식 최소 클라이언트는 Windows 10 1903이다. [Microsoft Win32 API](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createformonitor), [WGC 개요](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)

입력은 UIPI를 따른다. current-user 설치·일반 권한 실행을 기본으로 하고 관리자 앱을 제어하려고 자동 상승하지 않는다. 서명된 설치본, 설치/업데이트, 방화벽 안내, 실제 WGC/입력 검증은 배포 게이트에 남는다.

## iOS

현재 지원 범위 밖이며 이 작업에서 추가하지 않는다. 수신 Viewer 개발은 별도 플랫폼 작업이다. iOS 권한 준비를 Android/macOS 후보 완료 조건으로 넣지 않는다.

## 사용자가 수행할 다음 검증

1. 같은 source digest의 Host와 APK로 LAN 제어→미디어 연결, 짧은 실행 3회→10분→30분 순서로 수행한다.
2. 태블릿과 Galaxy XR에서 source/input 승인, 회전·비율·task 복귀·process death, 케이블 재연결을 확인한다.
3. macOS TCC 철회와 종료 후 캡처 해제를 확인한다. Windows는 별도 기기에서 설치·WGC·UIPI·재연결을 확인한다.
4. Android 기존 APK 업데이트 시 pairing/설정 유지, 백업/복원 시 재페어링과 저장 오류 안내를 확인한다. 자동 uninstall/reset으로 실패를 우회하지 않는다.

상세 절차와 정확한 후보 선택은 [완료와 지원 기준](completion-and-support.md), [Windows 검증](windows-remote-host.md), [persistent capture](macos-persistent-capture.md)를 따른다.
