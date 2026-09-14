# 2026-09-14 원래 Leftcar 앱의 스트림 수정과 실기기 검사

이 기록은 사용자 승인 후 원래 앱으로 전환한 후속 검사다. [이전 준비 기록](2026-09-14-completion-followup-validation.md)의 승인 대기·미설치·시험용 앱 계획은 당시 상태이며 현재 실행 방식을 설명하지 않는다. 사용자는 상세 검증 문서의 공개 GitHub 게시와 기존 실제 DP 화면의 TB710FU 공유를 승인했고, 원래 앱과 ADB 사용을 지정했다.

## 실행 방식

- macOS는 `/Applications/Leftcar Host.app`, Android는 `leftcar.ll3.kr`을 사용한다. 별도 benchmark 앱 식별자·상태 경로를 사용하지 않는다.
- macOS는 기존 Apple Development 서명과 designated requirement를 유지하며 정상 앱 종료 후 같은 경로를 업데이트한다. Android는 기존 인증서와 패키지로 `adb install -r` 업데이트한다. 페어링 데이터·화면 승인·Viewer 설정을 지우지 않는다.
- Android UI 조작과 화면 확인은 ADB로 수행한다. ADB는 조작·관찰 경로이고 실제 미디어 전송은 협상된 LAN 경로로 별도 기록한다.
- SHA-256은 소스·설치 파일·로그의 동일성을 확인하는 기록이다. 앱 식별자나 권한을 바꾸는 수단으로 사용하지 않는다.
- 새 화면 생성 기능은 비활성 상태를 유지한다. 외부 도구로도 화면을 생성하지 않으며 기존 모드·해상도·배율·배치·활성 상태를 바꾸지 않는다.
- 기존 물리 DP 화면의 로컬 애니메이션 패턴을 공유한다. 시스템 오디오와 원격 입력은 OFF다. 다른 프로젝트의 Android 앱과 데이터를 보존한다.

## 실제로 발견한 문제와 수정

| 문제 | 결과와 검증 범위 |
| --- | --- |
| Swift 미디어 AEAD 바이트 순서 불일치 | Rust/JS와 같은 `counter / ciphertext / tag`로 정렬했다. 실제 Swift와 Rust를 양방향 실행하는 고정 벡터·변조·재전송 검사를 로컬/CI gate에 연결했다. |
| Native launcher의 메서드가 객체 전개에서 누락 | TurboModule의 prototype/lazy 메서드와 receiver를 보존하는 명시적 위임으로 바꿨다. 관련 JS 회귀 검사와 타입 검사, React Doctor 100/100을 통과했다. |
| 준비 단계 인증 후 연결 완료 상태 누락 | 원래 앱에서 LCH1 인증·echo까지 성공한 뒤 IDR 요청과 피드백이 억제돼 약 6초 후 Host가 종료하는 것을 재현했다. 인증된 challenge가 renderer에 공유되는 연결 완료 상태를 설정하도록 수정했다. |
| 준비 단계가 일반 미디어의 replay counter를 소비 | challenge 분류는 인증만 수행하고 일반 미디어는 renderer에 남긴다. 활성 single/split과 준비·일시 정지 경로가 challenge 기록을 공유해 이미 처리한 challenge가 replay window를 초기화하지 않도록 했다. |
| 전용 제어 소켓의 암호문을 그대로 파싱 | 같은 미디어 crypto로 인증·replay 검사를 통과한 뒤 LCP2/ACK를 처리한다. 두 소켓 간 중복·변조·평문·잘린 패킷 거부와 실제 UDP 응답 회귀 4건을 확인했다. |
| Host 시스템 오디오가 최초부터 ON | 새 viewer는 명시적 SNDON 전까지 캡처하지 않는다. viewer별 소유권, ON/OFF, 마지막 세션 종료 후 초기화 회귀 검사를 통과했다. |
| 소켓 종료를 요청 크기 초과로 표시 | IO 오류와 실제 줄 상한 초과를 구분한다. 연결 reset 재현과 기존 상한 차단 검사를 유지했다. |
| CI 오디오 대기 테스트의 시간 의존 | 실제 대기 진입을 동기화해 생산 코드의 timeout을 바꾸지 않고 CI의 간헐 실패를 수정했다. |

첫 수정 후보의 실제 영상 검사는 실패했다. 이 실패를 단위 검사·빌드 성공으로 덮거나 30분 수락으로 계산하지 않는다. 실패 로그와 당시 소스·Host/APK는 별도 보관하며 후속 후보로 덮어쓰지 않는다.

최종 native 수정은 Android 라이브러리 **258/258**, Android 대상 컴파일과 Clippy를 통과했다. 독립 읽기 검토에서 발견한 active→suspend replay 경계를 수정하고 두 진입 API의 RED→GREEN을 확인했다. 마지막 독립 검토의 미해결 P0/P1/P2는 0이며, 실기기 수락과 구분한다.

통합 후 전체 Rust gate도 종료 0이다: workspace **425개**(기존 수동 검사 1개 ignored), Host **257 + 12개**, fmt·Clippy·구조 검사, Swift shim/기존 adapter 컴파일과 순수 split/RTX/오디오 정책·Swift↔Rust 암호화 상호운용 실행이 통과했다. 검사 전후 **896개 build input**, SHA-256 `465cbd2fa66fcb278872d5864f29f5fa8b4817c16c6fc5d9d84cbe6ca422f2a3`가 일치한다. React 변경은 앞서 JS **615개**·contract **4개**·타입/브라우저 검사·React Doctor **100/100**을 통과했고, 그 뒤 React 소스 변경은 없다. 수정된 검증 도구의 관련 검사 **8개**도 통과했다.

## 현재 실기기 수락 상태

**후속 후보 검증 중이다. 실제 1800초 수집은 아직 시작하지 않았다.** 새로운 후보로 짧은 시작/종료를 세 번 확인하고, 실제 움직이는 영상과 renderer 출력 증가가 확인된 뒤 60초 준비 구간을 거쳐 1800초를 별도로 수집한다. 패키지 생성 시간·앱 설정 시간·패턴 표시 시간은 포함하지 않는다.

최종 기록에는 정확한 소스 입력, Host/APK, 설치 인증서, 실행 프로세스, 화면 조건, 시작/종료 시각, 실제 경과 시간, encoder/Surface-release FPS, 중단·자원·열 상태와 로그 해시를 연결한다. Surface-release는 실제 패널 표시나 광학 입력 지연의 측정이 아니다.

## 공개 범위와 남은 경계

[공개 초안 PR #6](https://github.com/loopy-lim/leftcar/pull/6)에서 코드와 검증 문서를 제공한다. 원래 앱 업데이트 및 내부 서명 검증은 생산용 배포 서명·공증이나 공개 릴리스 완료를 뜻하지 않는다.

기존 의존성 조사에서 남은 Bun advisory 4건, Commons IO advisory 2건과 API 24/25 호환성 수락, Gradle lock/신뢰 정책·상시 scanner, NOTICE/라이선스 정책, 생산 서명·공증은 별도다. 물리 입력·오디오·USB·Galaxy XR·Windows와 60분 lifecycle 검사는 이번 단일 화면 30분 검사로 충족되지 않는다. 화면 생성 실험은 수행 대상에서 제외한다.
