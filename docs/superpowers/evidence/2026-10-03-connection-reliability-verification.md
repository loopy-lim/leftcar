# 연결 안정성 자동 검증

상태: 최종 소스에 대한 자동 검증 완료. 설치된 앱 및 실기기 E2E는 미검증.

범위는 [연결 안정성 수정 계획](../plans/2026-10-03-connection-reliability.md)이다. 사용자의 지시에 따라 설치된 앱의 E2E는 중단했다. 이후 새 앱 설치, OS 권한 변경, 기기 실행 검증을 수행하지 않았다. 아래 결과는 소스 및 격리된 자동 테스트의 증거이다.

## 수정한 연결 경계

- Viewer의 변경 명령은 응답 유실·시간 초과 후 자동 재전송하지 않는다. 읽기 요청만 닫힌 허용 목록에 따라 재연결 후 재시도한다. 쓰기 실패는 FIFO 소켓 전체를 폐기해 늦은 응답이 다른 요청을 완료하지 못하게 한다.
- 공유 재연결에서 한 호출자의 취소는 다른 창의 대기를 취소하지 않는다. Host 선택이 바뀌면 이전 연결 시도 자체를 취소한다.
- 복구·USB 전환·적응형 재구성 결과는 최초 선택과 정확한 창 생명주기를 확인한 뒤 적용한다. 오래된 성공 결과를 정리할 때 현재 창이 이미 소유한 세션은 중지하지 않는다. 종료 시 논리 소유권을 먼저 해제해 늦은 복구가 창을 다시 만들지 못하게 한다.
- 자동 복구는 창별 5회 예산을 공유한다. 네이티브 종료 알림과 Host 상태 조회는 같은 복구 컨트롤러를 사용한다. 내부 재생성이나 단순 시간 경과로 예산을 초기화하지 않는다. 결과가 불확실한 변경 명령, 취소 및 권한 거부는 자동 복구를 중단하고 명시적인 창 닫기·다시 연결을 안내한다.
- Android의 영상 없는 JNI 재부착은 6회로 제한한다. 실제 렌더 진행만 예산을 초기화하며, FPS 표시가 꺼져 있어도 진행을 관측한다. Surface 손실·세대 교체·종료 시 이전 복구 작업을 취소한다.
- React 부재 중 복구 알림은 활성 세대당 한 건으로 보류한다. 실제 JS 구독 등록 뒤 전달하고, 종료·세대 교체·영상 회복 시 제거한다.
- Android single/split 수신 경로는 인증 및 재전송 방지 검사를 통과한 패킷만 새 송신 포트와 연결 건강의 증거로 사용한다. 허용된 IP의 위조·재전송 패킷도 상태를 변경하지 못한다.
- Host는 소스 권한 철회 후에도 기존 키와 대상에 고정된 종료 알림만 제한 시간 안에 전송한다. 일반 미디어·입력·상태 송신의 권한 검사와 폐기 상태 검사는 유지한다. 내부 재구성 종료는 대체 Viewer를 종료하지 않는다.
- Mac/Windows TCP는 부분 프레임 이후 연결을 재사용하지 않는다. 전체 프레임의 제한 시간, EOF 및 실패한 쓰기를 처리한다. Windows와 Mac의 입력 상태는 인증된 heartbeat로 다시 전달된다.
- Host의 종료 세션 정리는 GUI 폴링 및 연결 허가 용량과 독립적이다. 주기 작업은 최대 한 건만 진행하고 종료·리스너 취소 후 새 작업을 시작하지 않는다.

## 검증

| 검사 | 결과 |
| --- | --- |
| React Doctor | 186개 파일 검사, **100 / 100**, 발견 사항 0 |
| 타입 / 전체 JS / 계약 테스트 | 타입 검사 통과, 76 files / 963 tests 및 계약 테스트 4개 통과 |
| 격리된 브라우저 회귀 | 16 suites / 116 cases 통과; 실제 연결·복구 컨트롤러 회귀 13개 포함 |
| Android JVM / 실제 Activity·Launcher·HUD | 31 suites, 173 tests, 실패·오류·skip 0 |
| Rust workspace | 467 tests 통과, 실패 0, 기존 ignore 2개 유지; Android native 289개 포함 |
| Host Rust / 실제 제어 서버 통합 | 357 + 13 tests 통과 |
| Swift 실제 세션 / 로컬 소켓 | TCP 3개, 종료 통지 4개, heartbeat 입력 상태 3개 통과; 검증 도구에서 컴파일 후 실행하도록 연결 |
| Rust Clippy / fmt / 구조 검사 | workspace 및 Host의 `clippy --all-targets -- -D warnings`, fmt, TS/Kotlin/Rust 구조 검사 통과 |
| Android Rust 대상 컴파일 | `aarch64-linux-android` 통과 |
| Windows 실제 backend 대상 타입 검사 | 통과; 플랫폼 중립 송신·TCP·입력 gate 경로는 실제 로컬 소켓 테스트 실행 |

각 핵심 결함은 기존 동작에서 실패하는 회귀를 먼저 확인했다. 실제 React 컨트롤러, ReservedStream, Kotlin Activity·Launcher, Rust 제어 서버와 송신기, Swift CaptureSession 경로를 사용했다. 테스트를 숨기기 위한 ignore나 기준 완화는 추가하지 않았다.

별도 검토자는 실제 Viewer 복구 호출 경로, Android single/split 인증 수신 경로, Mac TCP 실패 정리, Host 송신·입력 폐기 경계를 확인했다. 검토에서 발견된 결함을 수정한 뒤 해당 검토를 다시 통과했다.

최종 명령 로그는 `/private/tmp/leftcar-connectivity-*.log`, `/private/tmp/leftcar-connection-*.log`, `/private/tmp/leftcar-host-robustness-final-*.log`에 보관한다. 커밋 전 신규 파일의 EOF 빈 줄 한 건을 정리하고 React Doctor 100/100, 타입 검사, JS 963개 테스트를 다시 통과했다. 해당 로그는 `/private/tmp/leftcar-finalization-react-doctor.log`, `/private/tmp/leftcar-finalization-typecheck.log`, `/private/tmp/leftcar-finalization-js-tests.log`에 있다. 이 EOF 수정 외 플랫폼 검증 대상 소스는 동일하다.

source/config 840개 파일의 최종 검사 전후 해시 목록은 동일하다. 집계 SHA-256은 `b9636b665e88ff4fa31991b3b58e57f16b840d8177ef9f7d14306854aabc32e3`이며, 목록은 `/private/tmp/leftcar-finalization-source-before.json` 및 `/private/tmp/leftcar-finalization-source-after.json`에 있다. 기존 작업도 포함한 소스 스냅샷으로, 설치 산출물의 해시는 아니다. 신규 파일을 포함한 `git diff --cached --check`도 통과했다.

## 증거의 한계

- 설치된 Host/Viewer에 이 수정이 적용됐다는 증거가 아니다. 실제 LAN·USB 기기 연결, 패널 출력, 장시간 유지, 설치·배포는 이번 범위에서 검증하지 않았다.
- Windows 전체 Tauri 빌드는 `llvm-rc` 부재로 미검증이다. 실제 Windows backend 소스의 대상 타입 검사는 별도 수행했으나 Windows OS 실행 증거는 아니다.
- 추가 Android 대상 `clippy -D warnings`는 기존 JNI Safety 문서와 함수 인자 수 등 18개 lint로 통과하지 못했다. Android 대상 컴파일 및 workspace Clippy와 구분한다. 이 검사 기준을 숨기거나 완화하지 않았다.
