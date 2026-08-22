# macOS 지속 전체 화면 캡처

Leftcar Host는 원격 데스크톱 제품이므로 승인된 배포 빌드에서는 매 연결마다
화면 선택기를 띄우지 않고 지정한 전체 디스플레이를 직접 캡처하는 것을 목표로
한다.

## 실행 모드

- 실행 중인 프로세스에 `com.apple.developer.persistent-content-capture`가 실제로
  있으면 `SCShareableContent`에서 요청한 전체 디스플레이를 찾아 바로 캡처한다.
- 권한이 없으면 선택기를 띄우지 않고 `CGDisplayStream` 자동 화면 공유만
  광고한다. Viewer는 이 경로를 별도 선택 없이 기본값으로 사용한다.
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
가정하지 않는다. 승인 전 설치본은 `CGDisplayStream`, 승인된 설치본은
ScreenCaptureKit direct capture를 기본으로 사용하며 두 모드 모두 시스템 화면
선택기를 띄우지 않는다.

개발 중에는 저장소 루트에서 아래 명령만 사용한다.

```text
pnpm dev:host:macos
```

이 명령은 macOS 전용 Tauri 설정의 Apple Development 인증서로 앱을 빌드하고,
기존 `/Applications/Leftcar Host.app`과 designated requirement가 같은지 확인한
뒤 제자리에서 교체한다. 따라서 같은 Mac에서는 화면 기록을 최초 한 번만
허용하면 된다. 서명이 바뀐 빌드는 기존 권한을 잃지 않도록 설치 전에 차단한다.

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
