# 기술 타당성 조사

문서 상태: 조사 완료, 구현 가설은 미검증  
조사 기준일: 2026-08-17  
근거 정책: 플랫폼 설명은 공식 문서만 확정 근거로 사용하고, 성능은 실기기 관측 전까지 목표 또는 가설로 표시한다.

## 1. 결론

Leftcar의 핵심 제품은 현재 공개 API만으로 구현 가능한 범위에 있다.

- Galaxy XR Home Space는 여러 앱 창과 같은 앱의 여러 창을 열고 이동하고 크기를 바꾸는 사용 흐름을 제공한다.
- Android 15 이상은 앱이 multi-instance 시스템 UI를 지원한다고 manifest에 알릴 수 있다.
- 실제 다중 인스턴스 생성은 manifest property 하나가 아니라 Activity launch mode와 Intent flag로 결정된다.
- macOS ScreenCaptureKit은 디스플레이, 앱, 개별 창을 고성능으로 캡처한다.
- Windows.Graphics.Capture는 디스플레이 또는 앱 창을 선택하여 프레임을 얻는다. 여러 동시 capture session의 시스템 UI 동작도 문서화되어 있다.
- Android MediaCodec은 Surface로 하드웨어 디코딩할 수 있고, low-latency capability를 조회하고 활성화할 수 있다.
- Galaxy XR는 H.264, HEVC, VP9, AV1 재생과 Wi-Fi 7을 제품 사양으로 제공한다.

다만 다음은 아직 증명되지 않았다.

- Galaxy XR에서 Leftcar 같은 앱의 stream window 4개 이상이 동시에 안정적으로 열리는가
- H.264 decoder 4개를 동시에 1080p30으로 지속 실행할 수 있는가
- Home Space의 초점 없는 창이 계속 Surface를 갱신하고 표시하는가
- 창이 이동하거나 크기가 바뀔 때 Surface가 어떤 lifecycle로 재생성되는가
- Wi-Fi 7 연결 자체가 낮은 glass-to-glass 지연을 보장하는가
- QUIC custom video path와 WebRTC 중 어느 쪽이 실제 Galaxy XR에서 더 낮고 안정적인 지연을 보이는가
- macOS 앱 창을 가리거나 최소화했을 때 원하는 캡처 동작이 유지되는가
- 보호 콘텐츠, Metal overlay, HDR 창의 캡처 결과가 요구와 맞는가

이 항목들은 계획 초기에 실기기 spike로 판정한다.

## 2. “XR 기능 없이 여러 창”의 정확한 의미

### 2.1 가능한 모델

Android XR Home Space는 일반 Android 대화면/데스크톱 windowing 환경으로 취급할 수 있다. 같은 APK가 여러 task를 만들면 시스템은 각 task를 별도 창으로 표현할 수 있다.

Leftcar의 권장 구성:

```text
Leftcar APK
  HubActivity task               # 연결, source 목록, 진단
  StreamActivity task A          # Mac의 IDE 창
  StreamActivity task B          # Mac의 터미널 창
  StreamActivity task C          # Mac의 브라우저 창
  StreamActivity task D          # 두 번째 물리 디스플레이

Android XR Home Space
  [Hub] [IDE] [Terminal] [Browser] [Display 2]
```

각 task는 사용자가 독립적으로 이동, 크기 변경, 닫기 할 수 있다. task마다 `source_id`를 문서 URI 또는 saved state로 보유한다.

### 2.2 필요한 Android 설정

초기 manifest 가설:

```xml
<application
    android:resizeableActivity="true">

    <property
        android:name="android.window.PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI"
        android:value="true" />

    <activity
        android:name=".stream.StreamActivity"
        android:exported="false"
        android:launchMode="standard"
        android:documentLaunchMode="always" />
</application>
```

초기 launch 가설:

```kotlin
val intent = Intent(context, StreamActivity::class.java).apply {
    data = Uri.parse("leftcar://stream/$sourceId?instance=$instanceId")
    addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT)
    addFlags(Intent.FLAG_ACTIVITY_MULTIPLE_TASK)
}
context.startActivity(intent)
```

주의:

- `PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI`는 시스템 UI가 “새 창” 같은 affordance를 보여 주도록 알릴 뿐이다.
- 실제로 새 task가 생기는지는 Activity launch mode, document launch mode, Intent flag, document URI에 달려 있다.
- `singleTask`, `singleInstance`, 고정 task affinity는 이 목표와 충돌할 수 있다.
- 같은 source를 중복 창으로 열 수 있게 할지, 기존 창을 앞으로 가져올지는 제품 정책으로 정해야 한다. 기본은 source당 한 창이다.
- 시스템이 창을 복원할 때 페어링 비밀을 Intent에 넣지 않는다. `source_id`만 저장하고 세션 저장소에서 권한을 재검증한다.

### 2.3 독립 창과 독립 프로세스의 차이

각 창이 독립 task/Activity라고 해서 각각 별도 Android 프로세스는 아니다. 이 계획은 기본적으로 하나의 앱 프로세스를 공유한다.

공유할 것:

- 페어링 키와 보안 저장소
- Host와의 하나 또는 소수의 transport connection
- source catalog와 세션 상태 저장소
- 저빈도 Rustra engine
- telemetry aggregation

창마다 분리할 것:

- `StreamInstanceId`
- decoder 인스턴스
- 출력 Surface
- jitter/drop 정책
- 표시 profile
- Activity와 task lifecycle
- 사용자에게 보이는 오류 상태

별도 프로세스는 한 decoder crash의 격리를 높일 수 있지만 메모리, 키 전달, 공유 소켓, Rust runtime 중복, lifecycle 복잡도를 크게 늘린다. 첫 버전에서는 사용하지 않는다. 네이티브 decoder crash가 실제 반복 위험으로 확인될 때만 `android:isolatedProcess` 또는 별도 process 구조를 조사한다.

## 3. Android XR 선택지 비교

| 방식 | 여러 독립 창 | XR API | 다른 Android 앱과 공존 | 구현 위험 | 결정 |
| --- | --- | --- | --- | --- | --- |
| Home Space multi-instance Activity | 가능 | 불필요 | 가능 | 중간 | 기본 |
| 한 Activity 안의 타일 Overview | 앱 창은 1개 | 불필요 | 가능 | 낮음 | fallback/선택 기능 |
| Full Space SpatialPanel | 한 앱 내부 공간 패널 여러 개 | Jetpack XR 필요 | 다른 앱 숨김 | 높음, SDK 변동 | 후속 선택 |
| OpenXR/Unity | 자체 공간 렌더러 | OpenXR 필요 | Full Space 중심 | 매우 높음 | 제외 |
| 원격 앱마다 별도 APK | 가능 | 불필요 | 가능 | 설치/업데이트/보안 매우 복잡 | 제외 |

Home Space의 단점은 시스템이 창 배치와 수명 정책을 가진다는 점이다. Leftcar가 창의 정확한 3D 위치를 프로그램으로 강제하거나 곡면 배열을 만들 수는 없다. 하지만 사용자가 직접 배치하는 모니터형 사용에는 적합하다.

## 4. macOS Host 타당성

### 4.1 캡처 API

ScreenCaptureKit은 `SCShareableContent`로 디스플레이, 실행 중인 앱, 창을 열거하고 `SCContentFilter`로 단일 창 또는 디스플레이를 선택할 수 있다. `SCStreamConfiguration`은 크기, pixel format, 색공간, cursor, 최소 frame interval, queue depth를 제공한다.

권장 첫 경로:

```text
SCContentSharingPicker
  -> SCContentFilter(desktopIndependentWindow)
  -> SCStream
  -> CMSampleBuffer / IOSurface
  -> VideoToolbox VTCompressionSession
  -> encoded access unit
```

### 4.2 권한

- 화면 기록 권한은 사용자 동의가 필요하다.
- 시스템 content sharing picker를 우선 사용한다.
- 권한 거부와 철회를 정상 상태로 처리한다.
- 첫 권한 부여 뒤 재시작이 필요한 OS 동작을 runbook에 포함한다.
- source 승인과 Viewer 페어링 승인을 별도 단계로 둔다.

### 4.3 인코딩

VideoToolbox 가설:

- H.264 hardware encoder를 우선한다.
- `kVTCompressionPropertyKey_RealTime = true`
- `kVTCompressionPropertyKey_AllowFrameReordering = false`
- 짧은 keyframe interval과 즉시 IDR 요청을 지원한다.
- 가능한 OS에서 low-latency rate control capability를 확인한다.
- capture/encode 사이에서 CPU BGRA 복사를 피하고 IOSurface/CVPixelBuffer 경로를 유지한다.

주의: 속성 set 성공이 실제 하드웨어 인코더 사용 증거는 아니다. 세션 속성 조회와 Instruments/로그를 같이 확인한다.

### 4.4 미검증 macOS 경계

- 최소화된 창
- 완전히 다른 창에 가려진 창
- 다른 Space에 있는 창
- 전체 화면 앱
- 보호된 영상
- 120Hz ProMotion 디스플레이
- 여러 `SCStream`과 여러 `VTCompressionSession`의 동시 상한
- source resize 중 SPS/PPS와 decoder reconfiguration

## 5. Windows Host 타당성

### 5.1 캡처 API 우선순위

1. `Windows.Graphics.Capture`
   - 사용자 시스템 picker로 디스플레이 또는 앱 창 선택
   - `Direct3D11CaptureFramePool`로 GPU frame 획득
   - 여러 동시 capture session을 플랫폼 UX가 인식
2. Desktop Duplication API
   - 전체 디스플레이 fallback
   - dirty/move rect와 cursor metadata 제공
   - 앱 창 단위 기본 경로로는 사용하지 않음
3. Indirect Display Driver
   - 실제 가상 모니터가 꼭 필요할 때만 별도 프로젝트로 검토

### 5.2 인코딩

Media Foundation hardware MFT를 먼저 검증한다.

- `MF_LOW_LATENCY`와 `CODECAPI_AVLowLatencyMode`
- frame reordering 없음
- H.264 baseline/main/high profile 실기기 호환 비교
- D3D11 texture에서 encoder input까지 GPU 경로 유지
- adapter mismatch 시 copy 비용 계측

### 5.3 미검증 Windows 경계

- 최소화된 UWP/Win32 창
- exclusive fullscreen 게임
- protected content
- admin/UAC secure desktop
- HDR/10-bit surface
- 여러 GPU와 eGPU
- frame pool resize와 device lost

## 6. Linux Host 타당성

Linux는 v1 이후다. XDG Desktop Portal ScreenCast v6는 monitor, window, virtual source type, multiple selection, cursor mode, restore token, PipeWire remote를 문서화한다. 구현은 portal backend와 desktop environment에 따라 달라지므로 GNOME/Wayland와 KDE/Wayland를 별도 검증해야 한다.

Linux에서 portal의 `VIRTUAL` source가 보인다고 모든 배포판에서 가상 모니터를 동일하게 만들 수 있다고 가정하지 않는다.

## 7. Galaxy XR 디코딩 타당성

### 7.1 공식 장치 능력과 해석

Samsung의 Galaxy XR 사양은 H.264, HEVC, VP9, AV1 재생, 최대 8K60 재생 정보, Wi-Fi 7, 16GB RAM을 제시한다. 이는 유용한 상한 정보지만 다음을 보장하지 않는다.

- decoder 4개 동시 실행
- 4개 stream의 총 pixel rate
- low-latency mode 지원
- Home Space multi-instance 렌더링 중 thermal 지속 성능
- 네트워크 수신과 여러 Surface 합성까지 포함한 glass-to-glass 지연

따라서 Viewer는 실행 시 `MediaCodecList`를 조사해 다음을 기록한다.

- codec name과 canonical name
- MIME/profile/level
- hardware accelerated, software only, vendor 여부
- `FEATURE_LowLatency`
- `getMaxSupportedInstances()`
- supported performance points
- size/rate support
- color formats

`getMaxSupportedInstances()`는 상한 힌트이며 실제 성공 수가 더 적을 수 있다. performance point는 다중 codec에서 각 stream의 pixel rate 합으로 평가하고 실험으로 확인한다.

### 7.2 표시 Surface

첫 구현은 stream window마다 React Native Fabric `RemoteSurface` component 하나를 둔다. Android host view는 `SurfaceView`를 소유하지만 decoder는 Rust가 Android NDK `AMediaCodec`으로 구동한다.

```text
TypeScript <RemoteSurface instanceId=... />
 -> generated Fabric component
 -> thin Kotlin SurfaceView manager
 -> Surface jobject through JNI
 -> Rust ANativeWindow_fromSurface
 -> Rust AMediaCodec configure(output = ANativeWindow)
```

encoded frame, codec config, decoder loop는 Kotlin과 TypeScript로 전달하지 않는다.

이유:

- MediaCodec이 Surface에 직접 출력할 수 있다.
- 일반 영상에서 TextureView보다 전력과 frame timing에 유리하다는 Android 가이드가 있다.
- 각 Activity 창과 decoder가 일대일이라 복잡한 앱 내부 compositor가 필요 없다.

fallback:

- Galaxy XR Home Space에서 SurfaceView와 다중 창 합성 문제가 있으면 TextureView를 비교한다.
- 색 변환, custom scaling, 단일 compositor가 필요하면 decoder output을 HardwareBuffer/GL/Vulkan 경로로 받는 별도 spike를 연다.

### 7.3 lifecycle 핵심

Activity가 보이는 동안에도 focus를 잃을 수 있다. 멀티 윈도우에서는 여러 Activity가 동시에 resumed 상태일 수 있다. 따라서 재생을 `onPause()` 하나에 연결하지 않는다.

필요한 상태 입력:

- Activity started/resumed/stopped/destroyed
- window focus
- visible bounds와 density
- Surface created/changed/destroyed
- task removal
- process foreground/background
- display refresh rate

정책 가설:

- visible + Surface ready: 재생
- visible + unfocused: 재생 유지, profile만 낮출 수 있음
- stopped: decoder와 source를 짧은 grace period 뒤 suspend
- Surface destroyed: compressed queue를 비우고 새 Surface에서 IDR 요청
- task removed: 해당 source lease 해제

## 8. Rustra 적합성

현재 Rustra는 Rust에서 command를 정의하고 TypeScript client를 생성하며 Node, Bun, Tauri, React Native, Lynx adapter를 제공한다. 이 기능은 Leftcar의 UI와 native core 사이 제어 계약에 잘 맞는다.

적합한 payload:

- source 목록과 metadata
- 페어링 상태
- stream start/stop 요청
- 품질 profile 변경
- 1Hz 상태/통계 snapshot
- typed error
- 앱 설정

부적합한 payload:

- H.264/HEVC access unit
- 매 frame metadata event
- decoder input buffer
- pixel buffer 또는 HardwareBuffer handle의 TS 왕복

Rustra의 로컬 FFI/rkyv fast path 성능이 좋아도 영상 전체를 그 경로에 넣을 이유는 없다. JNI/JSI/TypeScript object 생성과 GC를 추가하고 backpressure 책임을 흐리기 때문이다.

또한 Rustra local command schema와 Host-Viewer network protocol은 별도 버전 계약이다. 같은 타입 이름을 공유할 수는 있지만 network wire format을 Rustra 내부 archive ABI에 암묵적으로 결합하지 않는다.

## 9. Viewer UI와 언어 선택

기본 개발 언어는 TypeScript와 Rust다.

- TypeScript: React Native 화면, Hub/stream window UI, Rustra generated client, 사용자 상태 표시
- Rust: domain, pairing, transport, packetization, quality, diagnostics, Android NDK decoder
- Kotlin: Android가 요구하는 Activity/TurboModule/Fabric/Surface lifecycle 접착만 담당

Kotlin shim에 허용되는 일:

- `HubActivity`와 `StreamActivity` 선언
- document task Intent 생성
- React Native host와 initial props 연결
- Fabric native component의 Surface 생성/파괴 callback 전달
- Java `Surface`를 JNI handle로 Rust에 전달
- Android lifecycle 신호 전달

Kotlin shim에 금지되는 일:

- 세션 상태 머신
- network 또는 cryptography
- packet assembly
- codec 선택/재시도/품질 정책
- source 권한 정책
- 사용자 오류 매핑
- business model 저장

Android NDK는 `AMediaCodec` decoder API와 `ANativeWindow`를 제공한다. Rust는 NDK C API binding을 통해 압축 access unit을 decoder에 넣고 Surface에 출력한다. Java `Surface`에서 `ANativeWindow`를 얻는 한 번의 JNI 경계만 Kotlin/Fabric shim이 제공한다.

React Native Codegen은 TypeScript spec에서 Android/C++ 연결 코드를 생성할 수 있다. Leftcar는 다음 spec을 source of truth로 둔다.

- `StreamWindowLauncherSpec.ts`: 새 document task를 여는 TurboModule
- `RemoteSurfaceNativeComponent.ts`: Surface를 소유하는 Fabric component
- Rustra generated control client: UI와 Rust core command

handwritten Kotlin은 작고 안정적인 adapter로 유지하고 line count 자체보다 책임 경계를 CI로 검사한다.

### 9.1 후보 비교

| 후보 | 장점 | 단점 | 계획 |
| --- | --- | --- | --- |
| React Native + Rust NDK video component | TS UI, Rustra RN adapter, TS Codegen spec, Rust hot path | 얇은 Kotlin/Fabric shim과 multi-instance 검증 필요 | 기본 |
| Native Kotlin/Compose + Rust JNI | Android windowing과 MediaCodec 제어가 직접적 | 팀 언어와 맞지 않고 Rustra의 TS 계약을 UI에서 쓸 수 없음 | 긴급 fallback만 |
| ReactLynx + Rust NDK video component | Rustra Lynx fast path와 TS UI | multi-instance Android host와 Fabric 대체 경계의 성숙도 검증 필요 | v1 제외 |
| Tauri Android | Rust core 연계 | Android XR window/video native 통합 불확실성이 큼 | 기본 제외 |

결정 원칙은 “TypeScript/Rust 중심을 유지하되, Android component 존재 자체를 숨기지 않는다”다. Phase 1에서 React Native multi-instance vertical slice가 실패하면 먼저 RN host/lifecycle 접착을 고친다. 전체 Kotlin shell 전환은 두 번의 기술 검토와 명시적 승인 없이는 하지 않는다.

## 10. 전송 후보

### 10.1 후보 A: WebRTC

장점:

- 실시간 영상용 congestion control, encryption, feedback, keyframe 요청이 검증된 계열
- 다중 video track 모델
- 네트워크 변화 대응과 NAT 확장 경로

단점:

- libwebrtc 빌드와 native 통합이 무겁다.
- 내부 jitter buffer와 playout 정책이 text-monitor use case에 맞지 않을 수 있다.
- 기존 RN WebRTC 모듈이 Galaxy XR multi-window와 원하는 low-latency 설정을 노출하지 않을 수 있다.

### 10.2 후보 B: QUIC stream + DATAGRAM

구성 가설:

- reliable bidirectional stream: handshake, control, codec config, IDR 요청
- QUIC DATAGRAM: video fragments
- reliable unidirectional stream: 선택적 IDR/config delivery

장점:

- Rust 양끝 구현과 세밀한 최신 프레임 우선 정책
- local LAN에 맞는 작은 jitter policy
- 하나의 연결에서 source별 logical channel

단점:

- packetization, loss recovery, congestion control 사용법, FEC, MTU, fragmentation, pacing을 직접 설계해야 한다.
- 잘못 구현하면 WebRTC보다 불안정하거나 네트워크에 불공정하다.
- Android native library와 decoder feeder를 직접 만들어야 한다.

### 10.3 선택 정책

문서 단계에서 승자를 정하지 않는다. Phase 2 bake-off에서 동일한 capture, encode, decode, source clip, network condition으로 비교한다.

필수 판정:

- p50/p95/p99 glass-to-glass
- 0/0.1/1/3% loss
- 0/10/30ms jitter
- Wi-Fi roam 또는 5초 단절 후 복구
- IDR recovery time
- CPU, memory, battery, thermal
- 1/2/4 stream
- 60분 latency creep

두 후보 모두 목표를 만족하면 구현/운영 복잡도가 낮은 쪽을 고른다.

## 11. 코덱 선택

### H.264

- 기본 baseline
- encoder/decoder 호환성이 높다.
- text content에서 4:2:0 chroma blur가 보일 수 있다.
- 높은 bitrate와 적절한 scaling filter로 먼저 품질을 평가한다.

### HEVC

- 같은 품질에서 bitrate 절감 가능성이 있다.
- encode/decode 지연, profile, 라이선스/배포, 하드웨어 동시 인스턴스를 검증해야 한다.
- H.264 baseline 이후 선택 profile이다.

### AV1

- Galaxy XR 재생 사양에는 포함된다.
- macOS/Windows Host의 실시간 hardware encode 가용성이 장치마다 다르다.
- v1 기본으로 두지 않는다.

### 4:4:4와 무손실

4:4:4, RGB 무손실, pixel perfect는 v1 약속이 아니다. 모바일 hardware decoder와 bandwidth/thermal 경로에서 일반적인 가정이 아니며 실기기 capability가 필요하다. text quality가 부족하면 다음 순서로 개선한다.

1. source native resolution과 scaling 점검
2. bitrate와 QP 조정
3. H.264 profile 비교
4. HEVC Main 10 또는 장치 지원 profile 비교
5. 부분 영역/중요 타일 adaptive quality
6. 4:4:4 capability spike

## 12. Phase 1 타당성 실험

| ID | 질문 | 최소 실험 | 통과 기준 | 실패 시 |
| --- | --- | --- | --- | --- |
| F-01 | 같은 앱 창 4개가 가능한가 | Galaxy XR에 색상/카운터가 다른 StreamActivity 4개 실행 | 4개 이동/resize/close, 모두 계속 animate | Overview 기본안 또는 Jetpack XR 재검토 |
| F-02 | 비초점 창도 Surface 갱신되는가 | 4개 SurfaceView에 독립 frame counter | 10분 동안 모두 진행 | lifecycle/Surface 타입 비교 |
| F-03 | decoder 4개가 가능한가 | Rust NDK `AMediaCodec`으로 H.264 golden stream 4개 동시 decode | 1080p30 4개, crash/지속 stall 없음 | F4 adaptive profile 또는 단일 compositor |
| F-04 | low latency mode가 있는가 | codec probe와 parameter enable | 지원 여부/실패 코드 기록 | 지원 없이 기준선 측정 |
| F-05 | Mac 창 4개 capture/encode 가능한가 | ScreenCaptureKit/VT 4 session | 각 1080p30, 프레임 timestamp 연속 | shared capture/encoder budget 재설계 |
| F-06 | multi-window resize가 안전한가 | 각 창 반복 resize 100회 | crash/leak/decoder deadlock 없음 | Surface lifecycle adapter 수정 |
| F-07 | RN/TS shell과 얇은 shim이 충분한가 | Rustra command + 2 stream windows + Rust `AMediaCodec` | addNumbers 42와 두 native Surface 동시, Kotlin에 정책 없음 | shim/RN host 구조 수정 후 재시험 |
| F-08 | transport 승자는 무엇인가 | 동일 clip의 WebRTC/QUIC bake-off | NFR과 복구 기준 비교 | 더 단순한 후보 선택 또는 범위 축소 |

## 13. 금지할 성급한 결론

- Galaxy XR가 8K60을 재생하므로 1080p60 네 개도 된다는 결론
- Wi-Fi 7이므로 지연이 10ms 아래라는 결론
- Rustra local FFI가 마이크로초이므로 glass-to-glass가 10ms라는 결론
- APK가 빌드되므로 Home Space multi-instance가 된다는 결론
- 에뮬레이터 네 창이 되므로 Galaxy XR 네 창도 된다는 결론
- 디코더 capability가 8 instances를 반환하므로 실제로 8개가 된다는 결론
- UI에 영상 사각형이 보이므로 hardware decode라는 결론
- software timestamp 합이 낮으므로 실제 디스플레이 photon 지연도 낮다는 결론

각 주장은 [검증 수준](README.md#검증-수준)에 맞는 증거를 요구한다.
