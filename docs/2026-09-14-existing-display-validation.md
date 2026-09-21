# 기존 디스플레이 검증 정책과 소스 확인 (2026-09-14)

사용자는 새 디스플레이·가상 디스플레이 생성 실험에서 화면이 멈춘다고 보고했고, 해당 기능을 비활성 상태로 유지하면서 남은 작업과 30분 검사를 진행하도록 요청했다. 이번 변경은 **이미 제거된 기능의 상태를 소스로 재확인하고 검사 정책을 명확히 한 문서 변경**이다. 새 기능 플래그를 추가하거나 가상 화면 기능을 이번에 다시 제거한 것이 아니다.

## 현재 검사 정책

- 이미 존재하며 사용자가 캡처를 승인한 디스플레이의 정확한 `sourceId`를 사용한다. 기존 사용자 화면·프로필·연결·설정을 보존한다.
- 새 디스플레이 생성, 기존 디스플레이 모드·배율·배치·활성 상태 변경을 검사 준비·복구·정리에 사용하지 않는다. BetterDisplay·CGVirtualDisplay·기타 외부 도구를 통한 가상 디스플레이 생성·크기 변경도 포함한다. 재개에는 사용자의 **새 명시적 승인**이 필요하다.
- 필요한 해상도나 source 수가 현재 구성에 없으면 해당 4K·다중 source 조합을 미검증으로 기록한다. 부족한 조건을 채우려고 화면을 만들거나 바꾸지 않는다.
- `reconfigureStream`의 인코딩 해상도 변경과 Viewer/XR 창 비율 변경은 Host 디스플레이 모드를 바꾸는 기능이 아니다. 실제 디스플레이 입력 크기와 인코딩 출력 크기를 각각 기록한다.
- 이전 계획·실험 문서의 가상 화면 생성·크기 변경 절차는 현재 실행 지시가 아니다. 과거 기록과 고정 baseline patch는 보존하며, 현재 실행에는 이 정책과 [지원 범위](completion-and-support.md)를 적용한다.

## 소스 감사

기준 소스: `091a9164a135d901325ba24d31c0736d0bbbcce5` (`completion-followup`). 런타임 소스와 계약·도구를 직접 읽고 검색했다. 이 감사는 설치된 바이너리나 실제 화면 상태를 검사한 결과가 아니다.

| 경로 | 확인한 동작 |
| --- | --- |
| [Host Tauri 등록](../apps/host-desktop/src-tauri/src/lib.rs) | 모듈 목록과 `generate_handler!`에 가상 디스플레이 생성·관리 명령이 없다. |
| [Host RPC](../apps/host-desktop/src-tauri/src/control.rs), [Rust 계약](../crates/control-contract/src/host.rs), [생성 계약](../packages/control-generated/host/commands.ts) | `startStream`과 `reconfigureStream`은 기존 source와 스트림 크기를 받는다. 가상 화면 생성·크기 변경 dispatch와 `viewerDisplay`/`virtualDisplayId` 계약은 없다. 나머지 명령은 등록된 stateless 계약으로 위임한다. |
| [macOS FFI](../apps/host-desktop/src-tauri/src/ffi.rs), [native exports](../native/macos-capture-shim/Sources/CaptureShim+Exports.swift) | 기존 디스플레이 목록·캡처 시작/정지 경계이며 관리 화면 생성·모드 변경 FFI가 없다. 캡처는 활성 디스플레이에서 선택한 ID를 사용한다. |
| [native 디스플레이 조회](../native/macos-capture-shim/Sources/Capture/CaptureBackend.swift), [source 선택](../native/macos-capture-shim/Sources/Capture/DisplaySourceSelection.swift) | `CGGetActiveDisplayList`, 현재 모드·후보 모드의 copy API와 AppKit backing 크기는 기존 화면의 조회에만 쓰인다. 화면 생성·모드 적용 API는 없다. source가 없거나 중복되거나 benchmark ID와 다르면 선택을 거부한다. |
| [Viewer 세션 크기](../apps/viewer-expo/src/use-catalog-model.ts), [스트림 제어](../apps/viewer-expo/src/launch-stream.ts), [상태 전환](../apps/viewer-expo/src/display-resize.ts) | 명시적 크기 변경은 `reconfigureStream`을 호출하고 세션의 인코딩 target을 갱신한다. Host 디스플레이 생성·크기 변경 요청은 없다. |
| [Android bridge](../apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt) | 스트림 준비·열기·닫기와 Viewer 창 비율 기능이며 제거된 디스플레이 생성/메트릭 전달 메서드는 없다. |
| [Windows 캡처](../apps/host-desktop/src-tauri/src/windows_backend/capture.rs), [입력](../apps/host-desktop/src-tauri/src/windows_backend/input.rs) | WGC `pool.Recreate`는 이미 바뀐 프레임 크기에 맞추는 버퍼 재생성이다. `SM_*VIRTUALSCREEN`은 기존 데스크톱 좌표 조회이며 가상 디스플레이 생성이 아니다. |
| [Host 패키징](../apps/host-desktop/src-tauri/tauri.macos.conf.json), [개발 실행 도구](../tools/dev-host-macos.zsh) | 현재 native 번들은 capture dylib이며 CGVD·BetterDisplay 생성 도구를 빌드하거나 묶는 경로가 없다. |

[2026-09-07 제거 기록](virtual-display-removal-validation.md)의 제거 범위와 현재 소스가 일치한다. 따라서 런타임 수정·새 플래그·문자열 존재 여부만 검증하는 새 테스트는 추가하지 않았다.

## 이번 확인의 범위

- 소스 감사와 문서·JSON 변경만 수행했다. 화면 생성·변경 명령, 앱 실행, 화면 캡처, 장치 명령, Host/APK 빌드는 실행하지 않았다.
- `bun run test apps/viewer-expo/src/launch-stream.test.ts apps/viewer-expo/src/display-resize.test.ts`: **2개 파일, 40개 테스트 통과**(2026-09-14, 종료 코드 0). 기존 `launch-stream.test.ts`는 실제 준비/요청 흐름의 payload에 제거된 가상 화면 필드가 없음을 검사하며, `display-resize.test.ts`는 세션 target 전환을 검사한다. 이 결과는 설치·실기기·30분 수용의 증거가 아니다.
- `tools/benchmark-profile.baseline068.json`의 실행 안내를 기존 승인 화면으로 수정했다. 고정 patch·baseline·파일 해시는 그대로 보존했다.
- JSON 파싱, 고정 baseline metadata 비교와 patch SHA-256 확인, 두 문서의 로컬 링크 확인, 변경분 whitespace 검사를 통과했다. 런타임·React 변경이 없어 이 문서 작업에서 빌드나 React Doctor를 다시 실행하지 않았다.
