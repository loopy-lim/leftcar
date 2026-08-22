# Leftcar 문서 인덱스

문서 상태: 제안안 0.1  
조사 기준일: 2026-08-17  
구현 상태: v0.1.0 구현, QR 페어링 인증 포함

## 먼저 읽을 결론

Leftcar의 첫 버전은 XR 애플리케이션이 아니다. Galaxy XR에서 실행되는 **멀티 인스턴스 일반 Android 앱**이다. Android XR의 Home Space가 같은 앱의 여러 task/Activity 인스턴스를 각각 독립적인 2D 창으로 보여 준다. 기본 매핑은 `원격 source 하나 = StreamActivity 창 하나`다.

이 결정은 다음 요구를 동시에 만족한다.

1. OpenXR, 시선 추적, 손 추적, 3D 렌더링, 공간 앵커를 사용하지 않는다.
2. macOS 또는 Windows의 여러 앱을 각각 별도 창으로 한 번에 볼 수 있다.
3. 영상 경로를 네이티브 캡처, 하드웨어 인코딩, 네트워크, 하드웨어 디코딩에 집중시킬 수 있다.
4. Rustra를 잘 맞는 위치인 로컬 제어 계약에 사용하고, 고대역폭 영상 전송 프레임워크처럼 오용하지 않는다.

Android XR의 공식 사용자 안내는 Home Space에서 같은 앱의 여러 창을 열고, 이동하고, 크기를 바꿀 수 있다고 설명한다. 따라서 별도의 SpatialPanel 없이도 사용자가 원하는 “각각의 앱이 돌아가는 형태”에 근접할 수 있다. 이 창들은 같은 APK의 독립 Activity/task이고, 반드시 별도 Android 프로세스인 것은 아니다. 네트워크와 인증은 공유 세션이 담당하고, 각 창은 자신의 decoder와 source lifecycle을 가진다.

Full Space의 SpatialPanel은 더 자유로운 3D 배치를 제공하지만 다른 Android 앱을 숨기고 XR SDK 의존성을 만든다. 기본 계획에는 들어가지 않는다.

## 문서 지도

| 순서 | 문서 | 답하는 질문 |
| --- | --- | --- |
| 1 | [제품 요구사항](01-product-requirements.md) | 무엇을 만들고 무엇을 만들지 않는가 |
| 2 | [기술 타당성 조사](02-feasibility-research.md) | Galaxy XR와 각 데스크톱 OS에서 실제로 가능한가 |
| 3 | [시스템 아키텍처](03-system-architecture.md) | 캡처부터 화면 표시까지 어떻게 나누는가 |
| 4 | [Rustra 제어 계약](04-rustra-control-contracts.md) | Rustra가 담당할 명령, 상태, 오류는 무엇인가 |
| 5 | [TDD와 품질 전략](05-tdd-quality-strategy.md) | 어떤 순서로 테스트를 먼저 작성하고 무엇을 증명하는가 |
| 6 | [성능 측정 계획](06-benchmark-device-validation.md) | 빠르다는 주장을 어떻게 재현 가능하게 측정하는가 |
| 7 | [보안과 개인정보 보호](07-security-privacy.md) | 화면 데이터와 페어링을 어떻게 보호하는가 |
| 8 | [24주 구현 계획](08-implementation-roadmap.md) | 다른 개발자가 어떤 순서와 완료 기준으로 구현하는가 |
| 9 | [위험과 미결정 사항](09-risk-register.md) | 어떤 가설이 실패할 수 있고 언제 방향을 바꾸는가 |
| 10 | [공식 근거 자료](10-references.md) | 계획의 플랫폼 근거는 어디에서 확인하는가 |
| 부록 | [Apple 화면 공유 비교 기준](apple-screen-sharing-baseline.md) | Apple UX·성능 기준과 Leftcar의 차이는 무엇인가 |

결정 기록:

- [ADR-0001: Home Space의 멀티 인스턴스 2D 앱](decisions/0001-home-space-multi-instance-viewer.md)
- [ADR-0002: 제어 경로와 영상 경로 분리](decisions/0002-separate-control-and-video-planes.md)
- [ADR-0003: 앱 창 스트림 우선](decisions/0003-window-streams-before-virtual-displays.md)
- [ADR-0004: 전송 방식은 실기기 대결 실험 후 확정](decisions/0004-transport-bakeoff-before-commitment.md)

실행 템플릿:

- [작업 인계 템플릿](templates/task-handoff.md)
- [벤치마크 결과 템플릿](templates/benchmark-result.md)

## 제품 이름과 용어

| 용어 | 이 문서에서의 뜻 |
| --- | --- |
| Leftcar Host | 화면을 캡처하고 인코딩하여 보내는 macOS 또는 Windows 애플리케이션 |
| Leftcar Viewer | Galaxy XR에서 스트림을 디코딩하고 보여 주는 Android 애플리케이션 |
| source | 캡처할 물리 디스플레이 또는 애플리케이션 창 |
| stream window | source 하나를 표시하는 독립 `StreamActivity` task. Home Space에서 별도 창으로 보인다 |
| overview tile | 선택적 Overview 창 안에 배치된 축소 원격 화면 하나 |
| panel | Jetpack XR의 공간 패널. 기본 제품은 사용하지 않는다 |
| control plane | 검색, 페어링, 세션, 품질, 상태, 오류 같은 저빈도 메시지 경로 |
| video plane | 압축된 영상 액세스 유닛을 전달하는 고빈도 바이너리 경로 |
| glass-to-glass | 호스트 화면의 픽셀 변화부터 Galaxy XR 디스플레이에 그 변화가 보일 때까지의 시간 |
| virtual display | OS가 실제 모니터처럼 인식하는 추가 논리 디스플레이 |
| window stream | OS의 특정 앱 창을 하나의 독립 영상 소스로 캡처한 것 |

## 요구사항 해석

사용자의 “모니터 여러 개, 즉 애플리케이션을 동시에 여러 개”라는 표현은 v1에서 다음과 같이 해석한다.

- 앱 창 A, 앱 창 B, 물리 디스플레이 C를 각각 독립 source로 선택할 수 있다.
- Viewer는 source마다 새 `StreamActivity` task를 열고, Android XR Home Space가 이를 각각 이동/크기 변경/닫기 가능한 창으로 표시한다.
- Hub 창을 닫더라도 열린 stream window는 유지될 수 있다. 모든 stream window가 닫히면 공유 세션이 유휴 상태로 전환된다.
- 각 stream window는 독립적으로 해상도, 프레임률, 비트레이트를 낮추거나 일시 중지할 수 있다.
- Overview는 선택 기능이며 1x1, 2x1, 2x2 타일 레이아웃을 한 창에 제공한다.
- v1은 호스트 OS에 새 가상 디스플레이를 만들지 않는다.
- 캡처 대상 앱은 Host가 해당 스트림을 `Control`로 전환했을 때만 Galaxy XR에서 조작한다. 기본 상태는 `Observe`다.

Android 시스템 창보다 더 자유로운 깊이 배치, 곡면 배치, 3D 고정이 필요할 때만 선택적 Spatial Mode를 검토한다.

## 검증 수준

문서와 구현 보고에서 다음 증거를 섞지 않는다.

| 수준 | 증거 | 주장할 수 있는 것 |
| --- | --- | --- |
| E0 | 설계 문서 | 의도와 가설이 정리되었다 |
| E1 | 단위 테스트 | 순수 로직과 상태 전이가 명세대로다 |
| E2 | 통합 테스트 | 프로세스 또는 FFI 경계가 테스트 환경에서 연결된다 |
| E3 | 빌드와 패키지 | 대상 플랫폼 산출물이 생성된다 |
| E4 | 에뮬레이터 | Android 앱과 일부 디코더 경로가 가상 환경에서 동작한다 |
| E5 | Galaxy XR 실기기 | 실제 코덱, Surface, lifecycle, 발열 조건에서 동작한다 |
| E6 | 실제 호스트와 실기기 종단간 | 캡처에서 표시까지 실제 픽셀이 전달된다 |
| E7 | 계측된 장시간 실험 | 지연, 드롭, 발열, 메모리 목표를 재현 가능하게 만족한다 |

`cargo test`, APK 설치, 검은 화면이 아닌 UI 표시는 E6나 E7의 증거가 아니다.

## 문서 변경 규칙

1. 구현 전에 관련 ADR 상태와 미결정 사항을 확인한다.
2. 플랫폼 동작을 추정으로 확정하지 않는다. 공식 문서와 실기기 결과를 같이 남긴다.
3. 수치는 “요구 목표”, “관측값”, “상한 추정”을 구분한다.
4. 벤치마크는 장치, OS, 빌드, 코덱, 해상도, 네트워크, 전원 및 온도 조건을 함께 기록한다.
5. 새 기능은 먼저 실패하는 테스트와 수용 기준을 추가한다.
6. 영상 프레임이나 페어링 비밀은 로그와 테스트 fixture에 넣지 않는다.
7. 한 작업은 한 명 또는 한 에이전트가 독립적으로 끝낼 수 있는 범위로 나눈다.

## 구현 시작 승인 체크리스트

다음 항목이 합의되기 전에는 Phase 1 이후로 진행하지 않는다.

- [x] 기본 UX가 Home Space의 멀티 인스턴스 stream window라는 점
- [x] 원격 입력은 기본 거부·Host 스트림별 승인이고, 오디오와 파일 전송은 제외한다는 점
- [x] macOS를 첫 호스트로 삼고 Windows는 두 번째로 진행한다는 점
- [x] 로컬 네트워크 직접 연결만 첫 배포 범위에 둔다는 점
- [x] Rustra는 제어 계약에만 쓰고 영상은 네이티브 별도 경로로 보낸다는 점
- [x] 전송 방식은 Galaxy XR 실기기 bake-off 결과로 결정한다는 점
- [x] v1은 가상 디스플레이 드라이버를 만들지 않는다는 점
- [x] 실제 Galaxy XR에서 같은 앱 창 4개를 동시에 열 수 있는지 Phase 1에서 검증한다는 점
