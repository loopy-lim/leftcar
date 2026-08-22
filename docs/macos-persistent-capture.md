# macOS 지속 전체 화면 캡처

Leftcar Host는 원격 데스크톱 제품이므로 승인된 배포 빌드에서는 매 연결마다
화면 선택기를 띄우지 않고 지정한 전체 디스플레이를 직접 캡처하는 것을 목표로
한다.

## 실행 모드

- 실행 중인 프로세스에 `com.apple.developer.persistent-content-capture`가 실제로
  있으면 `SCShareableContent`에서 요청한 전체 디스플레이를 찾아 바로 캡처한다.
- 권한이 없으면 `SCContentSharingPicker`를 `.singleDisplay`로 띄운다. 이 경우에도
  개별 창이 아니라 전체 디스플레이만 선택할 수 있다.
- 빌드 플래그가 아니라 `SecTaskCopyValueForEntitlement`로 실행 중인 앱의 실제
  권한을 검사한다.

## Apple 승인 후 활성화

1. Apple의 Persistent Content Capture Entitlement Request 양식을 제출한다.
2. 승인된 App ID와 provisioning profile에 capability가 포함됐는지 확인한다.
3. `PersistentCapture.entitlements.example`을 실제 entitlements 파일로 복사하고
   Tauri `bundle.macOS.entitlements`에 연결한다.
4. 앱을 다시 서명하고 설치한다.
5. 설치 앱의 권한을 확인한다.

```sh
codesign -d --entitlements :- "/Applications/Leftcar Host.app"
```

출력에 아래 값이 `true`로 있어야 persistent 경로의 실기기 검증을 시작한다.

```xml
<key>com.apple.developer.persistent-content-capture</key>
<true/>
```

승인 전에는 예제 entitlements를 활성화하거나 ad-hoc 서명으로 권한이 있다고
가정하지 않는다. 소스 컴파일은 검증할 수 있지만 선택기 없는 지속 캡처 성공은
승인된 프로필로 설치한 앱에서만 판정한다.

## 네이티브 shim 빌드

```sh
cd native/macos-capture-shim
swiftc -O -emit-library Sources/CaptureShim.swift \
  -o libleftcar_capture.dylib \
  -framework ScreenCaptureKit -framework VideoToolbox \
  -framework CoreMedia -framework CoreVideo -framework CoreGraphics \
  -framework IOSurface -framework Security -framework Foundation
```

`Security.framework`는 빌드 플래그가 아니라 현재 실행 프로세스에 서명된 실제
entitlement를 읽는 데 사용한다.
