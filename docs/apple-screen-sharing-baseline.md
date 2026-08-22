# Apple 화면 공유 비교 기준

검토일: 2026-08-22
목적: Apple 화면 공유의 공개 동작을 Leftcar 원격 입력과 고성능 스트리밍의 UX·안전성 기준으로 사용한다.

## 공식 기준에서 확인한 동작

- Apple 화면 공유는 `Observe Mode`와 `Control Mode`를 분리하며 연결 중에도 전환할 수 있다.
- High Performance 연결은 Apple silicon/macOS 14 이상에서 30/60fps, 낮은 지연, 4:4:4, HDR, 가상 디스플레이를 지원한다. Apple은 4K 한 화면에 75Mbps와 일관된 낮은 네트워크 지연을 권장한다.
- High Performance 세션은 대상 Mac의 로컬 `Escape` 키로 종료할 수 있다. FaceTime 원격 제어에서는 대상 사용자의 로컬 입력이 원격 입력보다 우선한다.
- Apple 공개 문서는 마우스·트랙패드의 내부 polling rate를 명시하지 않는다. 따라서 Leftcar의 `영상 FPS × 2` 정책은 Apple 수치의 복제가 아니라 독립적인 지연 목표다.

공식 자료:

- [Share the screen of another Mac](https://support.apple.com/guide/mac-help/share-the-screen-of-another-mac-mh14066/mac)
- [Screen sharing type options on Mac](https://support.apple.com/guide/mac-help/screen-sharing-type-options-on-mac-mchl1883115d/mac)
- [Request or give remote control in FaceTime on Mac](https://support.apple.com/guide/facetime/request-or-give-remote-control-fctmebd8481a/mac)

## Leftcar 반영 상태

| 기준 | Leftcar 상태 | 판정 |
| --- | --- | --- |
| Observe/Control 명시 전환 | 모든 스트림은 Observe로 시작하고 Host가 세션별 Control을 켠다 | 구현 |
| OS 권한과 세션 승인 분리 | macOS는 손쉬운 사용 권한, Windows는 UIPI 경계와 Host 스트림 토글을 요구한다 | 구현 |
| 고주파 입력 경로 분리 | Kotlin → JNI → Rust → 인증 UDP → CGEvent/SendInput이며 TS/Rustra/JSON을 통과하지 않는다 | 구현 |
| 포인터 전송률 | 30fps→60Hz, 60fps→120Hz, 90fps→180Hz, 최대 240Hz 목표 | 구현·실측 대기 |
| Android 입력 배치 회피 | API 30 이상 마우스 소스에 unbuffered dispatch를 요청하고 Rust에서 최신 위치만 유지한다 | 구현 |
| 이산 입력 신뢰성 | 키·버튼·휠·전체 해제는 sequence/ACK/20ms 재시도를 사용한다 | 구현 |
| 포커스/연결 종료 안전 | 포커스 이탈, Surface 종료, transport 재연결, Host 토글 OFF에서 전체 해제한다 | 구현 |
| 대상 Mac 로컬 입력 우선 | Host 토글로 즉시 회수 가능하나 시스템 전역 로컬 입력 우선순위는 아직 없다 | 후속 |
| 로컬 Escape 긴급 해제 | Dashboard가 아닌 시스템 전역 단축키는 아직 없다 | 후속 |
| Dynamic Resolution/가상 디스플레이 | 현재 실제 display capture와 고정 소스 해상도 사용 | 범위 밖 |
| 4:4:4/HDR/오디오 | 현재 H.264 4:2:0 영상 전용 | 범위 밖 |
| 공유 Clipboard/파일 전송 | 의도적으로 제공하지 않는다 | 기본 거부 |

## 실기기 수용 기준

1. 60fps 스트림에서 포인터 송신률 목표 120Hz, 90fps에서 180Hz를 네이티브 카운터 또는 패킷 캡처로 계측한다. 입력 장치가 목표보다 낮은 report rate이면 장치 report rate를 상한으로 함께 기록한다.
2. 5% 입력 데이터그램 유실과 500ms 단절 뒤에도 키·버튼이 눌린 상태로 남지 않아야 한다.
3. 영문/한글 전환, Shift/Control/Option/Command, 방향키, F1–F12, 숫자패드, key repeat를 실제 Mac 앱에서 확인한다.
4. 단일·다중 디스플레이의 네 모서리와 Retina scaling에서 포인터 오차를 확인한다.
5. Control OFF, Android 창 포커스 이탈, Activity 종료, Wi-Fi 경로 재연결 각각에서 전체 해제를 확인한다.
6. Apple 화면 공유 Standard/High Performance와 동일 Mac·동일 네트워크에서 체감 및 계측 결과를 별도 기록한다. Apple의 비공개 입력 polling rate를 추정값으로 단정하지 않는다.
7. 같은 Viewer 입력 장치와 네트워크에서 Windows Host의 120/180Hz wire rate, UIPI 제한, 다중 모니터 모서리 좌표를 별도 기록한다.
