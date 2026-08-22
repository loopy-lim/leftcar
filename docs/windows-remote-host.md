# Windows 원격 Host 구현과 검증

검토일: 2026-08-22

이 문서는 Windows x64 PC를 Leftcar Host로 사용해 Galaxy XR/Android Viewer에서 화면을 보고 키보드와 마우스로 제어하는 경로의 구현 범위와 검증 경계를 기록한다.

## 지원 범위

| 영역 | 구현 | 실패 정책 |
| --- | --- | --- |
| Host shell | 동일한 Tauri 2 앱, mDNS/QR pairing/control server | Windows backend 초기화 실패 시 테스트 backend로 전환하지 않고 앱 시작 실패 |
| source catalog | `EnumDisplayMonitors`의 활성 물리 디스플레이 | 시작 시 index를 다시 확인해 hot-plug stale index 거부 |
| capture | `IGraphicsCaptureItemInterop::CreateForMonitor` + WGC `CreateFreeThreaded` frame pool | 5초 안에 첫 frame이 없거나 resize/recreate 실패 시 session error |
| encode | BGRA → NV12, Media Foundation H.264 MFT | `MFT_ENUM_FLAG_HARDWARE` 결과가 없으면 명시 실패, software fallback 없음 |
| media | SPS/PPS `CFG`, Annex-B H.264, 1,200-byte 이하 UDP fragment | malformed output/fragment 초과 시 명시 실패, send queue drop 시 IDR 요청 |
| input | 인증 UDP → 별도 worker → `SendInput` | Observe가 기본, session별 Control OFF/종료에서 key/button 전체 해제 |
| package | Windows x64 current-user NSIS installer | CI 산출물은 서명 전 internal artifact이며 public release로 간주하지 않음 |

Windows Graphics Capture desktop interop의 monitor 생성 API는 Windows 10 version 1903부터 제공되므로 Leftcar Windows Host의 최소 기준도 Windows 10 1903으로 둔다.

## 입력 지연 정책

- Android Viewer는 포인터 위치를 영상 FPS의 2배로 샘플링한다: 30fps→60Hz, 60fps→120Hz, 90fps→180Hz, 전체 범위 30–240Hz.
- pointer move는 최신 좌표만 유지하고 reliable queue를 막지 않는다.
- key, button, wheel, release-all은 순서 번호와 인증된 ACK를 사용하며 20ms 간격으로 재시도한다.
- Windows input worker는 WGC/Media Foundation worker와 분리한다. 캡처 readback이나 encoder가 늦어져도 UDP 입력 수신과 ACK가 같은 작업 큐에서 기다리지 않는다.
- absolute pointer 좌표는 선택한 monitor 좌표를 Windows virtual desktop 좌표로 변환한다. 다중 DPI/회전/배율의 물리 검증은 아직 E6 항목이다.

`SendInput`은 UIPI의 적용을 받는다. 일반 권한 Leftcar Host는 같거나 낮은 무결성 수준의 앱을 제어할 수 있지만 관리자 권한으로 실행된 앱에는 입력을 넣을 수 없다. Leftcar는 이 제한을 우회하거나 자동으로 관리자 권한을 요구하지 않는다. 또한 Windows 문서가 지적하듯 `SendInput`은 현재 keyboard state를 초기화하지 않으므로 Leftcar가 실제로 주입한 down transition만 추적하고 Control OFF/종료 시 해당 항목만 release한다.

## Viewer/Host capability 협상

`getCatalog` 응답에는 다음 정보가 포함된다.

```json
{
  "platform": "windows",
  "captureBackends": [
    {
      "id": "windowsGraphicsCapture",
      "label": "Windows Graphics Capture",
      "hint": "권장 · Media Foundation 하드웨어 H.264"
    }
  ],
  "displays": []
}
```

Viewer는 이 목록의 첫 backend를 기본으로 사용한다. 따라서 Windows 연결에서 macOS 전용 `screenCaptureKit` 문자열을 보내지 않는다. 오래된 Host와 연결할 때만 ScreenCaptureKit fallback 표시를 사용한다.

## 빌드와 CI

Windows PC:

```text
pnpm install --frozen-lockfile
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests -- -D warnings
pnpm --filter @leftcar/host-desktop tauri build
```

CI의 `windows-host` job은 동일한 테스트와 clippy를 실행한 뒤 unsigned NSIS `*-setup.exe`를 artifact로 올린다. 설치 모드는 current-user라서 관리자 권한을 요구하지 않는다. 공개 배포에는 별도의 Windows code-signing gate가 필요하다.

macOS에서 가능한 소스 수준 검증:

```text
rustup target add x86_64-pc-windows-msvc
RC=/path/to/x86_64-w64-mingw32-windres \
  cargo check --manifest-path apps/host-desktop/src-tauri/Cargo.toml \
  --target x86_64-pc-windows-msvc --lib
```

이 교차 `cargo check`는 Windows API symbol/type과 cfg 경계를 검증하지만 WGC frame, GPU codec 선택, installer 실행을 증명하지 않는다.

## 물리 Windows 수용 기준

1. Intel/AMD/NVIDIA GPU별로 실제 선택된 H.264 MFT identity를 기록하고 software encoder가 아님을 확인한다.
2. 1080p60 단일 stream과 1080p30 네 stream에서 첫 화면, FPS, bitrate, drop, CPU/GPU 사용량을 기록한다.
3. display resize, mode change, hot-plug, sleep/wake, source close에서 crash 없이 session error 또는 재시작으로 수렴해야 한다.
4. Windows→Android 실제 화면 전달, IDR recovery, Viewer 종료의 E6 시나리오를 수행한다.
5. 60fps/90fps에서 포인터 wire rate 120/180Hz와 입력 p50/p95를 측정한다.
6. 일반 앱과 관리자 앱을 각각 제어해 UIPI 제한이 UI 진단과 일치하는지 확인한다.
7. 다중 monitor의 네 모서리, 음수 origin, 서로 다른 DPI scaling, 화면 회전에서 pointer 오차를 측정한다.
8. 5% input datagram loss와 500ms 단절 뒤 stuck key/button이 없어야 하며, 60분 soak에서 handle/memory 증가가 없어야 한다.

현재 저장소에서 달성한 것은 protocol/unit test와 macOS→Windows MSVC 교차 compile까지다. 실제 Windows installer와 E6/E7 결과는 Windows CI 및 물리 장치 실행 뒤에만 달성으로 변경한다.

## 공식 근거

- [Microsoft: Screen capture](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)
- [Microsoft: IGraphicsCaptureItemInterop::CreateForMonitor](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createformonitor)
- [Microsoft: H.264 Video Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder)
- [Microsoft: MFT enumeration flags](https://learn.microsoft.com/en-us/windows/win32/api/mfapi/ne-mfapi-_mft_enum_flag)
- [Microsoft: SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Tauri: Platform-specific configuration](https://v2.tauri.app/reference/config/#platform-specific-configuration)
- [Tauri: Windows installer](https://v2.tauri.app/distribute/windows-installer/)
