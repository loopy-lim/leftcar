# Sidecar 사용성·전송 품질 개선 제안

상태: 사용자 설계 승인, 병렬 구현 진행 중. 구현 완료 문서가 아니다.
기준: 2026-09-05, main HEAD 47236ce. 기존 미추적 docs/tablet-cursor-streaming-validation.md는 변경하지 않았다.

## 제품 우선순위

1. 입력과 화면 반응 지연을 최소화한다.
2. 읽을 수 있는 글자 크기와 매끄러운 윤곽을 유지한다.
3. 영상 재생에서 프레임 간격과 복구를 안정화한다.
4. 가상 디스플레이 추가·배치 및 Android/XR 창 비율을 제공한다.

## 현재 확인한 사실

- 실제 macOS 인코더에 RealTime=true, AllowFrameReordering=false가 설정되어 있다. Android 디코더에도 low-latency 요청이 있다. 설정 존재는 장치에서의 성능 입증이 아니다.
- apps/viewer-expo/src/adaptive-resolution.ts의 fallbackTargetFor는 정확한 3840×2160에만 2560×1440 폴백을 만든다. 16:10 및 세로 화면은 제외된다.
- 같은 파일의 isCongested는 receiverLossDelta > 0을 필수로 요구한다. 손실 없는 인코더 처리량 저하나 큐 지연만으로는 낮추지 않는다. 정지 화면의 낮은 FPS를 혼잡으로 오인하지 않는 설계가 필요하다.
- clarity 프로필은 allowUpscale=true다. 원본 확대와 macOS의 실제 HiDPI 렌더링을 구분해야 한다.
- BetterDisplay 및 CGVD shim 모두 HiDPI를 끈다. 기존 실패 사례를 피하려고 픽셀 모드 하나로 고정한 계약이므로 단순 토글 변경은 부적절하다.
- 가상 디스플레이 생성/제거와 확장 모드가 존재한다. 검토한 코드에서 macOS 디스플레이 위치 변경 경로는 발견하지 못했다.
- Android StreamActivity는 다중 인스턴스와 크기 조절을 선언하며 Surface는 aspect-fit한다. StreamLauncherModule은 원본 비율의 창 크기를 요청하지 않는다. 영상 내부 비율 유지와 OS 창 크기는 별개다.
- CGVD shim은 고정 serialNum을 사용한다. 복수 화면 지원 시 식별자와 개별 수명 관리도 함께 검토해야 한다.

## 접근안 비교

| 안 | 장점 | 비용 및 한계 |
| --- | --- | --- |
| 기존 UDP/FEC·인코더 경로 개선 + 기존 VD 프로바이더 확장 (권장) | 현재 제품을 유지하며 단계별 비교 가능 | 수신 측 신호와 디스플레이 계약 보강 필요 |
| 항상 4K/HiDPI 고정 | 설정이 단순함 | 디코더·네트워크 부하로 최우선 지연 요구를 해칠 수 있음 |
| 전송과 XR UI 전면 재작성 | 더 넓은 설계 자유 | 현 문제와 직접 관계없는 범위 및 검증 부담 증가 |

## 권장 구현 범위

### 전송

- 원본 비율을 유지하는 해상도 단계와 짝수 픽셀 정렬을 도입한다. 글자 가독성을 위한 하한과 안정 구간·재전환 대기 시간을 둔다.
- 지속적인 큐 증가, 움직임이 있는 구간의 처리량 저하, 수신 디코더 상태를 구분한다. 정지 화면 저FPS만으로 강등하지 않는다.
- 기존 커서 분리 옵션의 종료·복구 경로를 실측하고 확인된 결함을 고친다. 검증 전 기본값 승격은 하지 않는다.
- 무조건적인 인코더 변경보다 동일 콘텐츠/기기/연결에서 전후 측정을 우선한다.

### 글자·HiDPI

- 논리 작업 공간과 실제 렌더 픽셀을 구분한다. 예: 논리 1600×1000, 2배 backing 3200×2000.
- 생성 후 실제 모드의 논리 크기와 pixelWidth/pixelHeight를 확인한다. 요청 실패 시 명확한 오류 또는 검증된 1배 모드로 복귀한다.
- 원본보다 큰 전송 확대의 실효성을 검토하고 실제 HiDPI 원본을 우선한다.
- BetterDisplay 기본 경로를 유지하며 CGVD 실험 경로도 동일한 크기 의미를 갖게 한다.

### 디스플레이·창

- macOS 가상 화면의 추가/개별 제거 및 좌·우·위·아래 배치를 제공한다. Leftcar 소유 화면을 고유 ID로 추적한다.
- Android/XR에서는 소스 비율에 맞는 최초 창 크기를 플랫폼 지원 범위에서 요청한다. 사용자가 크기를 바꾼 후에도 Surface와 입력 좌표는 같은 aspect-fit 변환을 사용한다.
- XR 공간의 창 이동은 시스템의 창 조작을 기본으로 사용한다. 임의 3D 공간 배치를 위한 Full Space 전환은 별도 범위다.
- 기존 단일 확장 모드와 복수 가상 화면의 소유권·정리 정책을 분리해 다른 화면을 삭제하지 않는다.

## 검증

- 실행 완료: bun run typecheck 통과, bun run test 30파일/346테스트 통과. 이 수치에는 .superpowers/sdd의 과거 스냅샷 테스트 149개가 포함되므로 현재 제품 테스트 수로 해석하지 않는다.
- 실행 완료: Host Rust virtual_display 관련 테스트 6개 통과. 실제 디스플레이 생성 검증은 아니다.
- 변경 후: 관련 TS/Rust/Swift/Kotlin 회귀 검사, Android/Host 빌드. React 변경 시 루트 React Doctor 100/100 후 타입 검사와 관련 테스트 재실행.
- 실기: 정지 글자, 스크롤, 창 이동, 60fps 영상에 대해 같은 조건으로 기준선과 개선 후 비교. 30초 탐색 → 180초 확인 → 10분 안정성 측정. 기존 E7 판정에 필요한 60분 soak는 별도로 수행한다.
- 기록: 렌더 FPS, 프레임 간격 p95, capture age p95, 큐 나이, decoder/output drops, frame gaps, IDR 복구, 연결 방식과 빌드 SHA. Capture age를 입력 지연이나 glass-to-glass 지연으로 표기하지 않는다.
- 글자 비교는 같은 논리 크기의 한글/영문/색상 글자를 1배와 HiDPI에서 비교한다. Galaxy XR 가독성과 공간 창 조작은 해당 기기에서 별도 확인한다.

## 현재 실측 제약

- adb devices -l: 기기 없음. adb mdns services: 발견 없음. 무선 디버깅 주소와 포트가 필요하다.
- 제한된 셸에서는 디스플레이가 열거되지 않았으나, 제한 밖 읽기 전용 재확인에서 M1 Max의 DP 외장 화면(backing 3840×2160, 논리 1920×1080, 60Hz)이 정상 연결됨을 확인했다. AppleClamshellState=Yes와 활성 화면 없음은 동치가 아니다.
- BetterDisplay CLI 실행 파일과 실행 중 앱을 확인했다. HiDPI 생성 성공은 구현 에이전트가 별도 검증한다.

## 외부 계약 참고

- BetterDisplay CLI: https://github.com/waydabber/BetterDisplay/wiki/Integration-features%2C-CLI
- Android XR manifest: https://developer.android.com/reference/androidx/xr/runtime/manifest/ManifestProperty

실제 구현 전 사용 SDK와 설치된 BetterDisplay CLI의 지원 계약을 다시 확인한다.
