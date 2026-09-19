# 트랙패드·가운데 클릭·선명한 화면 후속 검증

## 현재 요구사항과 상태

- **최신 고동작 시험은 불합격**이다. 4K 평균 출력 59.61 FPS와 구간 내 새 gap/복구 0회는 유지했지만 비트레이트 하한 압력과 지연 증가가 나타났다. 아래의 단순 화면 1분 통과는 기본 동작 확인으로만 사용한다.
- 최소 해상도는 **1440p**다. 1080p로 내려서 성능 문제를 해결하거나 합격 처리하지 않는다.
- 사용자의 일반 입력 성공 의견 이후 두 손가락 스크롤, 휠 누르기(가운데 클릭), 선명한 화면 실패가 추가로 보고됐다. 앞선 일반 사용 의견을 전체 물리 입력 합격으로 사용하지 않는다.
- 클릭 위치와 조작 결과는 필요한 화면 캡처로 확인한다. 반복 성능 측정과 입력 판정은 코드, 테스트, Host API와 Android 텍스트/JSON 로그로 효율적으로 진행한다.
- 두 손가락 스크롤과 선명한 화면 수정 후보를 일반 앱에 설치했다. 새 설치본에서 **3840×2160 분할 영상 출력과 프레임 카운터 증가**를 확인했다. 물리 장치 조작 확인은 아직 남아 있다.
- 가운데 클릭은 Android Activity의 기본 전달 테스트를 통과했다. **실물 MX Anywhere 3의 증상 원인은 아직 확정하지 않았다.**

## 두 손가락 스크롤

기존 코드는 일반 마우스의 `ACTION_SCROLL`과 H/V 휠 축만 처리했다. Android 14 이상은 두 손가락 스크롤을 `CLASSIFICATION_TWO_FINGER_SWIPE`로 분류한 MOVE와 `AXIS_GESTURE_SCROLL_X_DISTANCE/Y_DISTANCE`로 전달할 수 있다. 기존 경로는 이 동작을 커서 이동으로 전송했다.

실제 Activity와 Surface 계층에 해당 이벤트를 전달하는 테스트에서 기대한 스크롤 `(4, 0)` 대신 커서 이동 `(1, 0)`이 전송되는 실패를 확인했다. 분류된 제스처를 전용 스크롤 경로로 보내고, 현재 값과 모든 과거 묶음의 상대 이동량을 합산한다. 제스처 시작·종료가 클릭으로 변하지 않도록 소비한다. 기존 터치 스크롤의 픽셀→줄 변환 비율을 사용한다.

공식 의미와 단위: [Android MotionEvent](https://developer.android.com/reference/android/view/MotionEvent). 축은 누적값이 아닌 표본별 화면 픽셀 이동량이므로 묶음 기록을 모두 처리해야 한다.

## 선명한 화면

실기 로그에서 3840×2160 분할 디코더를 성공적으로 생성한 뒤 다음 불일치로 양쪽 타일을 종료했다.

```text
requested=OMX.qcom.video.decoder.avc.low_latency
actual=c2.qti.avc.decoder.low_latency
```

기기의 MediaCodec 목록에서 앞의 이름이 뒤의 이름을 가리키는 하드웨어 별칭임을 확인했다. 능력 조회와 생성 요청에 같은 정식 이름을 사용하도록 변경했다. 엄격한 네이티브 디코더 이름 검사와 소프트웨어 디코더 제외 정책은 유지한다.

별칭을 우선 나열하는 실제 MediaCodecList 테스트에서 수정 전 실패, 수정 후 성공을 확인했다. [Android MediaCodecInfo](https://developer.android.com/reference/android/media/MediaCodecInfo)의 정식 이름 API를 사용한다.

## 가운데 클릭 및 실기 확인 방법

기기 목록에서 MX Anywhere 3은 `BTN_MIDDLE`을 지원한다. Activity 테스트는 가운데 버튼의 눌림/해제를 각각 버튼 값 4로 보존했다. 이 두 사실만으로 실제 버튼 사용이 정상이라고 판정하지 않는다.

새 후보에는 화면 Surface에 들어오는 포인터 정보만 기록하는 진단 기능이 있다. 기본 비활성이고, 활성 구간당 최대 128개 기록으로 제한한다. 키 입력 내용이나 포인터 좌표는 기록하지 않는다.

```sh
adb -s 192.168.0.19:5555 shell setprop log.tag.LeftcarPointer DEBUG
adb -s 192.168.0.19:5555 logcat -v epoch -s LeftcarPointer:I '*:S'
# 실기 검증 후 진단 끄기
adb -s 192.168.0.19:5555 shell setprop log.tag.LeftcarPointer INFO
```

JSON에는 장치 ID, 입력 출처, action, 바뀐 버튼/눌린 버튼, 제스처 분류, 묶음 수, 휠과 제스처 이동량을 기록한다. 실제 입력과 앱 변환 경계를 확인하기 위한 기능이며 성능 측정 중에는 끈다.

로컬 입력 확인 페이지도 실제 스크롤할 내용, 스크롤 위치/높이, `auxclick` 기록을 추가했다. 이벤트 도착만으로 스크롤 성공을 선언하지 않고 위치 변화를 확인한다. 새 페이지 내용이 적용됐는지도 로그의 `scrollHeight` 필드로 판정한다.

초기 2분 장치 수집은 영상 세션이 닫힌 상태였고 입력 기록이 없었다. 이를 기기 입력 실패나 통과의 증거로 사용하지 않는다. 다음 실기 구간은 새 설치본에서 1440p 또는 선명한 화면을 연 뒤 시작해야 한다.

## 설치·자동 검증 근거

- APK: `artifacts/input-performance-2026-09-16/leftcar-touchpad-clarity.apk`
- APK 및 설치된 파일 SHA-256: `14eae6db440cbaa8233cc6bcca62f6300c1f62fb46b66a054779495e24fe228c`
- Android release 테스트 **125개 통과**, 실패·오류·누락 0.
- React Doctor **100 / 100**.
- 일반 패키지 덮어쓰기 설치 성공. 별도 비교 앱은 추가하지 않았다.
- 기존 작업 트리의 변경도 포함한 빌드다. 새 성능 결과가 나오기 전에는 이전 APK의 성능 수치를 새 후보의 합격 근거로 사용하지 않는다.
- `touchpad-clarity-build-receipt.json`: 소스·APK·설치 해시와 테스트 결과.
- `codec-inventory.jsonl`, `reported-input-clarity-failures.log`: 기기 코덱 목록과 4K 종료 원인.
- `leftcar-canonical-codec-red.log`, `leftcar-touchpad-red.log`: 수정 전 실패.
- `leftcar-touchpad-codec-green.log`, `leftcar-touchpad-clarity-build-tests.log`: 수정 후 회귀·전체 테스트.
- `latest-user-acceptance-criteria.json`: 1440p 최소 기준과 실제 보고된 세 가지 실패.

## 새 설치본의 실기 결과

### 입력과 창 유지

- `candidate-4k-pointer-framework.json`: 실제 TB710FU의 Android 프레임워크에서 합성한 가운데 클릭이 Mac 페이지의 `pointerdown`, `pointerup`, `auxclick`에 각각 도착했다. 두 손가락 분류 제스처 10개는 휠 이벤트 10개로 도착했고, 실제 페이지 스크롤 위치가 0→400px로 변했다. 이는 실제 기기의 소프트웨어 경로 검사이며 실물 장치를 눌렀다는 증거는 아니다.
- `candidate-4k-main-closed.json`: 메인 Task 508을 제거한 뒤 영상 Task 509가 유지됐다.
- `candidate-4k-basic-main-closed.json`: 첫 기본 검사에서 첫 KeyA DOWN만 Mac 페이지에 없었다. 나머지 키 유지·수정키·방향키·좌우 클릭·휠·드래그는 통과했다. 페이지 이벤트 sequence는 연속이므로 단순 JSON 파일 순서 문제로 처리하지 않는다.
- `candidate-4k-basic-repeat.json`: 동일 검사를 바로 반복했을 때 9개 항목을 모두 통과했다. 최초 누락 원인은 미확정이며 반복 성공으로 지우지 않는다.
- `candidate-4k-input-stress-main-closed.json`: 키 눌림/해제 50쌍, 좌클릭 51쌍, 드래그 MOVE 120개가 도착했고 최종 좌표도 허용 오차 안이었다. 메인 창이 없는 상태에서 같은 영상 Task를 유지했다.
- `physical-pointer-live-window.json` 및 `physical-*-live.log`: 후속 120초 수집에서도 물리 입력 기록이 없었다. 이를 물리 장치 성공이나 실패로 판정하지 않는다. 수집은 종료했고 포인터 진단 속성은 INFO로 되돌렸다.

### 4K 성능

`candidate-clarity.jsonl`, `candidate-clarity-gate.json`, `candidate-clarity-metrics.json`에 기록했다. 새 APK의 세션 10, 동일 분할 렌더러 incarnation에서 61.536초·62개 상태 표본을 수집했다. 클릭용 화면 확인은 측정 전 끝냈고, 측정 중에는 포인터 진단 로그를 껐다. Host와 Android의 시계 차이는 별도 경계 표본으로 맞춰 네이티브 로그 구간을 선택했다.

| 항목 | 결과 |
| --- | --- |
| 해상도 / 목표 | 3840×2160 / 60 FPS |
| 캡처 카운터 평균 | 59.96 FPS |
| 양쪽 Surface 출력 카운터 평균 | 59.91 FPS |
| 출력 상태 표본 | 중앙값 59, 최소 57 FPS; 62/62 표본이 57 FPS 이상 |
| 새 프레임 gap / 복구 키프레임 / 송신 실패 | 0 / 0 / 0 |
| 양쪽 출력 짝 불일치 폐기 | 3회 |
| 네트워크 RTT 표본 p95 | 11ms |
| 캡처→디코더 입력 지연 표본 p95 | 48ms (평활화된 값) |
| 캡처→양쪽 Surface 출력 지연 | 측정 없음 |

FPS·구간 유지·누락·복구·송신 기준은 이 구간에서 통과했다. **전체 지연 기준의 합격은 보류한다.** 현재 분할 지연 값은 디코더 입력 시점에서 끝나므로 48ms를 화면 표시 지연으로 바꾸어 부르거나, 후속 단계의 p95와 더해 전체 p95로 계산하지 않는다. Surface 출력 역시 실제 패널 발광 시점은 아니다.

화면이 보이고 포커스된 로컬 페이지의 작은 이동 막대가 시험 부하다. 고동작 영상·게임·장시간 사용·XR 동시 사용이나 Parsec 비교 결과가 아니다. 측정 구간 밖에는 복구 이력이 존재하므로 이 1분 결과를 전체 세션 무결점으로 확대하지 않는다.

남은 검증: 실물 두 손가락 스크롤과 가운데 클릭, 최초 키 DOWN 누락의 원인, 1440p 이상에서 실제 표시까지의 지연 기준. 현재는 메인 창을 닫은 채 4K 영상 창을 유지했다. 커밋과 push는 수행하지 않았다.

## 전체 화면 고동작 3분 후속 시험

사용자의 지적에 따라 작은 이동 막대의 결과를 일반 안정성으로 확대하지 않고, 같은 설치본·4K 세션에서 전체 화면이 계속 바뀌는 부하로 다시 측정했다. 별도 Android 비교 앱을 설치하지 않았다.

시험 장면은 재현 가능한 seed 20260916의 WebGL 합성 영상이다. 세밀한 질감, 배경 이동·회전·확대, 서로 다른 방향으로 움직이는 도형을 3840×2160 전체 화면에 그린다. 실제 영화나 게임을 대표한다고 주장하지 않는다. 클릭 위치와 장면이 그려지는지는 최소 화면 확인으로 검증했고, 반복 수치는 JSON과 네이티브 로그로 수집했다.

- 원본: `debug/high-motion.html`, 파일 해시·부하 계약: `high-motion-workload-receipt.json`.
- 원본 그리기 기록: `motion-source-cadence.jsonl`.
- 측정 원본: `candidate-high-motion.jsonl`, `candidate-high-motion-native.log`, 시계 경계: `candidate-high-motion-boundaries.json`.
- 결과: `candidate-high-motion-gate.json`, `candidate-high-motion-metrics.json`.

| 항목 | 단순 화면 | 전체 화면 고동작 |
| --- | --- | --- |
| 측정 구간 | 61.54초 | 181.40초 |
| 원본 RAF/그리기 빈도 | 59.98 FPS | 59.97 FPS |
| 양쪽 Surface 출력 카운터 평균 | 59.91 FPS | 59.61 FPS |
| 57 FPS 이상 상태 표본 | 62/62 | 178/181 |
| 캡처→디코더 입력 지연 표본 p95 | 48ms | **105ms** |
| 새 수신 gap / 복구 키프레임 / 송신 실패 | 0 / 0 / 0 | 0 / 0 / 0 |
| 비트레이트 하한 압력 카운터 증가 | 0 | **26** |

고동작 구간은 원본 4K 전체 화면·실행·포커스·가시성을 유지했고 WebGL 오류와 context loss가 없었다. 원본 그리기 간격의 p95는 20ms, 최대 38ms다. RAF는 원본 제출 빈도이며 물리 패널 FPS가 아니다.

실제 전송량 표본 중앙값은 약 16.35Mbps, p95는 19.74Mbps였다. 목표 비트레이트는 이 구간에서 **12.6–16.77Mbps**를 오갔다. 앞선 단순 화면의 설정은 60Mbps였다. 고동작 시험은 장면 전환과 준비 이후 수집을 시작했으므로 전환 직후의 전체 하락 과정을 이 구간이 포함하지는 않는다.

하한 압력 26은 서로 다른 화면 끊김 26회가 아니다. `CaptureSession+AdaptiveBitrate.swift`의 카운터는 혼잡 판단이 이어지고 비트레이트 제어가 하한에 닿아 해상도 대체를 요구한 관찰 횟수다. 측정 내 해상도는 계속 3840×2160이었다. 인코딩 이전 admission 카운터 증가 6,146도 큐를 꺼내기 전 차단된 시도 수이며, 잃은 고유 영상 프레임 수로 해석하지 않는다. 실제 캡처 큐 폐기는 27회, 출력 짝 불일치 폐기는 3회였다.

**판정: 고동작 안정성 기준 불합격.** FPS와 연결 유지가 좋아 보여도 낮아진 비트레이트 예산과 지연 증가를 숨기지 않는다. 화질 차이는 아직 픽셀 비교로 측정하지 않았고, 캡처→최종 표시 및 물리 입력→화면 지연도 미측정이다. 3분은 장시간 합격 근거가 아니다.

시험 후 고동작 부하를 중지하고 기존 입력 확인 페이지로 복귀했다. 다음 성능 개선은 고동작에서의 인코딩·전송 대기와 비트레이트 하한 압력을 분리해 다룬다. 1080p로 낮춰 합격시키지 않는다.
