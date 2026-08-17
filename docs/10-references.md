# 공식 근거 자료

확인일: 2026-08-17  
정책: 기술 및 플랫폼 사실은 가능한 한 원 제작사/표준 문서를 사용한다. 이 목록의 존재는 Leftcar 실기기 성능을 증명하지 않는다.

## 1. Rustra

- [loopy-lim/rustra GitHub](https://github.com/loopy-lim/rustra)
  - Rust command에서 TypeScript client 생성
  - Node, Bun, Tauri, React Native, Lynx adapter
  - local control contract의 근거
- 조사 시 public `main` HEAD: [`11ff71f5e2b5a0c563d50525eef82b0a05768c1f`](https://github.com/loopy-lim/rustra/commit/11ff71f5e2b5a0c563d50525eef82b0a05768c1f)
  - 구현자는 최신 상태를 다시 확인하고 vetted commit/tag를 pin해야 한다.
  - 조사자의 로컬 `feat/event-sink` branch는 public main보다 앞선 commit을 포함했으므로 계획이 그 branch를 암묵적으로 요구하지 않는다.

## 2. Android XR와 Home Space

- [Android XR Foundations](https://developer.android.com/design/ui/xr/guides/foundations)
  - Home Space는 여러 앱이 나란히 실행되는 multitasking 공간
  - 일반 mobile/large-screen Android app이 추가 개발 없이 동작 가능
  - Home Space에서 spatial panel은 지원하지 않음
  - Full Space는 한 앱이 중심이고 다른 앱이 숨겨짐
- [Spaces & multitasking on Android XR](https://support.google.com/android-xr/answer/16638859)
  - Home Space에서 여러 앱 창을 열고 이동/resize
  - 같은 앱의 여러 창을 계속 열 수 있다는 사용자 안내
- [Transition between Home Space and Full Space](https://developer.android.com/develop/xr/jetpack-xr-sdk/transition-home-space-to-full-space)
  - 기본 Home Space와 optional Full Space launch property
- [Android XR Fundamentals: Spaces and Spatial Panels](https://developer.android.com/codelabs/xr-fundamentals-part-1)
  - Home Space의 한 app window와 Full Space spatial panels 차이
- [Package and distribute apps for Android XR](https://developer.android.com/develop/xr/package-and-distribute)
  - Android XR dedicated track와 manifest feature

## 3. Android multi-instance/windowing

- [Support desktop windowing](https://developer.android.com/develop/adaptive-apps/guides/support-desktop-windowing)
  - Android 15 `PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI`
  - 새 task가 새 window로 열리는 주의점
- [WindowManager PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI](https://developer.android.com/reference/android/view/WindowManager#PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI)
  - property는 System UI만 제어
  - 실제 multi-instance는 Activity launch mode와 Intent flags에 달림
- [Multitasking and multi-instance](https://developer.android.com/guide/topics/large-screens/multitasking-and-multi-instance)
  - 같은 앱의 여러 instance를 별도 movable/resizable window로 실행
- [Adaptive do's and don'ts](https://developer.android.com/develop/adaptive-apps/guides/adaptive-dos-and-donts)
  - resizable/multi-window/adaptive layout 지침

## 4. Galaxy XR

- [Samsung Galaxy XR support specifications](https://www.samsung.com/us/support/answer/ANS10007499/)
  - 3552x3840 micro-OLED, XR2+ Gen2, 16GB RAM, Wi-Fi 7
  - H.264, HEVC, VP9, AV1와 8K60 재생 사양
  - battery 일반 사용/미디어 시간
- [Galaxy XR product specifications](https://www.samsung.com/uk/xr/galaxy-xr/galaxy-xr-silver-shadow-sm-i610nzsaeub/)
  - video codec와 connectivity 상세
- [Android Developers: Galaxy XR launch](https://developer.android.com/blog/posts/giving-your-apps-a-new-home-on-samsung-galaxy-xr-the-first-device-powered-by-android-xr)
  - Galaxy XR가 Android XR 기반이며 기존 2D Android 앱 적응 경로가 있다는 공식 소개

장치 사양은 단일/다중 decoder의 Leftcar 지속 성능 보장이 아니다.

## 5. Android codec와 Surface

- [MediaCodec low-latency feature](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities#FEATURE_LowLatency)
- [Android 11 low-latency MediaCodec](https://developer.android.com/about/versions/11/features#low-latency)
  - `FEATURE_LowLatency`, `KEY_LOW_LATENCY`, `PARAMETER_KEY_LOW_LATENCY`
- [Media codec performance](https://developer.android.com/media/optimize/performance/codec)
  - hardware/software/vendor identity와 performance points
- [CodecCapabilities.getMaxSupportedInstances](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities#getMaxSupportedInstances())
  - concurrent instance 상한 힌트이며 실제 수는 더 적을 수 있음
- [VideoCapabilities](https://developer.android.com/reference/android/media/MediaCodecInfo.VideoCapabilities)
  - size/rate와 performance point, 다중 codec 주의
- [Media3 Surface types](https://developer.android.com/media/media3/ui/surface)
  - 일반 영상에서 SurfaceView의 전력/frame timing 장점
- [Android NDK Media](https://developer.android.com/ndk/reference/group/media)
  - `AMediaCodec`, hardware/software codec identity, buffer API
- [Android NDK Native Window](https://developer.android.com/ndk/reference/group/a-native-window)
  - Java Surface와 대응하는 `ANativeWindow`
- [Android NDK Native Activity/JNI window](https://developer.android.com/ndk/reference/group/native-activity)
  - Java Surface에서 ANativeWindow 획득

## 6. React Native native boundary

- [React Native Native Platform](https://reactnative.dev/docs/native-platform)
  - Turbo Native Modules와 Fabric Native Components
- [Fabric Native Components](https://reactnative.dev/docs/next/fabric-native-components-introduction)
  - TypeScript spec과 Codegen으로 C++/Java 연결 코드 생성
- [Turbo Native Modules](https://reactnative.dev/docs/turbo-native-modules-introduction)
  - TypeScript spec 기반 native module codegen
- [Using Codegen](https://reactnative.dev/docs/the-new-architecture/using-codegen)
  - Android Gradle codegen task와 generated output

이 문서는 Kotlin이 불필요하다고 말하지 않는다. Android implementation shim은 필요하지만 TypeScript spec과 Rust core 중심으로 범위를 최소화한다.

## 7. macOS capture/encode

- [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
  - 고성능 screen/audio capture, fine-grained source, system picker, permission
- [Capturing screen content in macOS](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos)
  - display/window filter, SCStream configuration, permission sample
- [SCStreamConfiguration](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration)
  - width/height/pixel format/color/cursor/queue depth/frame interval
- [VideoToolbox compression properties](https://developer.apple.com/documentation/videotoolbox/compression-properties)
- [Real-time compression](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_realtime)
- [Disable frame reordering](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_allowframereordering)
- [Encoding video for live streaming](https://developer.apple.com/documentation/videotoolbox/encoding-video-for-live-streaming)

## 8. Windows capture/encode

- [Windows screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
  - `Windows.Graphics.Capture`, display/window picker, multiple capture session border
- [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
  - display frame, dirty/move rect, cursor, multi-monitor rotation
- [MF_LOW_LATENCY](https://learn.microsoft.com/en-us/windows/win32/medfound/mf-low-latency)
  - Media Foundation low-latency pipeline attribute
- [CODECAPI_AVLowLatencyMode](https://learn.microsoft.com/en-us/windows/win32/medfound/codecapi-avlowlatencymode)
  - codec low-latency and frame reordering expectation

## 9. Linux 후속 경로

- [XDG Desktop Portal ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
  - monitor/window/virtual source, multiple selection, cursor, restore, PipeWire remote
- [XDG Desktop Portal PipeWire](https://flatpak.github.io/xdg-desktop-portal/docs/pipewire.html)
  - portal-mediated PipeWire access control

## 10. 전송 표준 후보

- [WebRTC publications](https://www.w3.org/groups/wg/webrtc/publications)
- [WebRTC 1.0](https://www.w3.org/TR/webrtc/)
- [RFC 9000: QUIC](https://www.rfc-editor.org/rfc/rfc9000)
- [RFC 9221: QUIC DATAGRAM](https://www.rfc-editor.org/rfc/rfc9221)

표준의 존재는 특정 Rust/Android 구현의 품질을 보장하지 않는다. 실제 후보 library와 build chain은 bake-off 작업에서 pin하고 검증한다.

## 11. 근거 갱신 규칙

다음 시점에 링크와 핵심 해석을 다시 확인한다.

- G1 전에 Galaxy XR OS update 후
- Android target/compile SDK 변경 시
- React Native major 변경 시
- Rustra pin 변경 시
- macOS/Windows 최소 버전 변경 시
- release candidate 전

변경된 플랫폼 동작은 ADR과 위험 등록부에 반영한다.

