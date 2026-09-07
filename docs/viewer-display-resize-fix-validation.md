# 태블릿 반응 지연 및 화면 크기 변경 검증

작성일: 2026-09-07 KST. 기준 HEAD: `769d65d0390d` + 현재 작업 트리.
기기: Lenovo TB710FU, Wi-Fi ADB, Android H.264 Qualcomm 저지연 디코더 두 개.
작업은 Loop/Z.AI 우선으로 분담했고, 통합 수정·실기 검증은 순차 실행했다.
원본 기록 `docs/tablet-cursor-streaming-validation.md`는 수정하지 않았다.

후속 입력·동작 검증에서 영상 손실 후 최대 약 2.9초 복구 정지가 재현됐다.
일반 데스크톱 10분 결과를 전체 안정성 완료로 확대하지 않는다.
자세한 내용: [입력·화면 반응 후속 검증](tablet-input-response-validation.md).

## 수정 내용

- `resizeVirtualDisplay` Host dispatch를 연결하고 관리 ID, 논리 크기, backing 크기와 scale 응답을 검증했다.
- 뷰어 시작·재연결 시 `viewerDisplay`와 `virtualDisplayId`를 보존한다. 태블릿 크기 매칭은 가로 방향으로 정규화한다.
- 직접 입력 크기의 HiDPI 계산을 프리셋과 통일했다. 최초 scale은 추측하지 않고, Host가 확인한 값만 저장한다.
- 4K split에서 작은 해상도로 변경할 때 Host가 계산한 Auto 모드를 실제 교체 인코더에도 전달한다. 이전에는 세션 기록만 바뀌고 인코더는 SplitVertical로 시작했다. 뷰어도 응답 경로의 실제 모드를 저장한다.
- H.264 QTI 디코더에 `vendor.qti-ext-dec-low-latency.enable=1`, `vendor.qti-ext-dec-picture-order.enable=1`을 적용한다. 거부되면 같은 디코더를 표준 설정으로 재시도한다. 다른 제조사·HEVC·MIME 선택 경로에는 vendor 설정을 넣지 않는다.
- split 수신→입력→출력 준비→release 제출과 gap→IDR 입력→복구 출력 시간을 로컬 단조 시계로 측정한다. IDR 입력 전의 오래된 출력은 복구 성공으로 계산하지 않는다. 기존 Host 비트레이트 정책에 쓰이는 wire stale-frames 값은 바꾸지 않았다.
- Host 복구 대기 중 새 캡처가 없으면 보관한 최신 프레임을 복구 입력으로 사용할 수 있다. 재제출에는 새 sequence/generation과 단조 증가 VT PTS를 쓰고 원래 캡처 시간은 보존한다. 종료 시 보관 프레임을 정리한다.

## 반응 지연 A/B 실측

같은 구 Host 프로세스를 유지한 채 Android 디코더 설정 전후를 비교했다.
수치는 각 1초 로그의 최근 표본 P95를 모은 **중앙값**이며, 전체 프레임의 통합 P95가 아니다.
아래 표는 왼쪽 tile이며 초기 10개 로그를 제외했다. 전 299개/298.6초, 후 68개/67.1초 표본이다.
오른쪽 tile의 같은 디코더 구간도 218.663ms→20.301ms로 감소했다.

| Viewer 내부 구간 | 변경 전 | 변경 후 |
| --- | ---: | ---: |
| 수신→디코더 입력 | 5.418ms | 3.714ms |
| 디코더 입력→출력 준비 | **223.109ms** | **20.724ms** |
| 출력 준비→release 제출 | 4.871ms | 5.641ms |
| 수신→release 제출 | 227.396ms | 24.422ms |

디코더 구간에서 약 90% 감소했다. 전후 모두 `c2.qti.avc.decoder.low_latency`를 사용했다.
표준 low-latency 설정은 변경 전에도 수락되었다. 변경 후 picture-order 설정 수락을 확인했다.
codec의 output.delay 메타데이터는 이후에도 17이므로 그 값만으로 실제 지연을 해석하면 안 된다.

이 수치는 입력 이벤트→화면 표시의 전체 지연이 아니다. 네트워크 전송 시간, Host 캡처 시각과의
동기화, Surface release 이후 실제 패널 표시 시간은 포함하지 않는다. 사용자 체감 개선 확인은 별도다.

## 실기 크기 전환

수정한 Host 앱과 APK를 설치한 뒤 동일 세션에서 다음을 확인했다.

- 4K split→1920×1080: 단일 인코더로 정상 전환, Host 캡처/출력 60FPS. 이전 feedback timeout 실패가 재현되지 않았다.
- 1080p→4K 요청: 연결은 유지되었으나 Auto 단일 인코더 경로가 유지되어 이후 2560×1440으로 자동 하향되었다. **4K 복귀 후 4K60 유지 성공으로 처리하지 않는다.**
- 새로 시작한 4K UDP 스트림은 기존 capability 기반 자동 선택으로 SplitVertical을 사용한다.

Android에서 카탈로그와 스트림을 번갈아 전체 화면으로 가리면 Surface 종료와 피드백 타임아웃이
발생할 수 있다. 위 크기 전환은 두 창을 모두 보이게 하여 이 수명주기 요인을 분리했다.
관리 가상 화면은 이 환경에 없어 관리 ID 경로의 실제 resize는 수행하지 않았다.

## 최종 설치본 10분 연속 측정

2026-09-07 02:47–02:57 KST, Host 세션 5 / Viewer PID 20854.
일반 데스크톱 캡처이며 고정된 영상 부하 시나리오는 아니다. 측정 중 추가 빌드·설치·기기 조작은 하지 않았다.
양쪽 worker가 각각 동일 thread를 유지하고 rendered 누계가 계속 증가한 621.32초 표본이다.

| 항목 | 왼쪽 | 오른쪽 |
| --- | ---: | ---: |
| rendered 누계 기반 평균 FPS | 59.396 | 59.287 |
| 1초 로그 최저 FPS | 45 | 44 |
| 0FPS / 30FPS 미만 로그 수 | 0 / 0 | 0 / 0 |
| 최대 로그 간격 | 1.01초 | 1.013초 |
| 디코더 구간 최근 P95의 중앙값 | 20.210ms | 19.407ms |
| 수신→release 제출 최근 P95의 중앙값 | 25.567ms | 23.783ms |

Host 표본은 처음부터 끝까지 3840×2160 SplitVertical running, 최종 capture/encode 60FPS였다.
네트워크 gap·복구와 일시적인 40FPS대 저하는 있었다. 따라서 모든 프레임이 60FPS이거나 모든 끊김이
제거됐다는 뜻은 아니다. 이 표본에서 이전과 같은 0FPS 정지는 관측하지 못했다.
`pairedResumes`는 coordinator의 양쪽 IDR 수신 기준이며 실제 패널 표시 완료 지표가 아니다.

영수증: `android-final-continuous.log`, `final-continuous-summary.json`, `status-timeline.jsonl`.
측정용 로그 수집은 종료했고 앱과 4K 스트림은 실행 상태로 남겼다.

## 자동 검증 및 설치

| 검사 | 결과 |
| --- | --- |
| React Doctor, 최종 TS 수정 후 | **100 / 100** |
| 저장소 typecheck | 통과 |
| 웹 Vitest | **34파일, 396개 통과** |
| android-viewer 호스트 단위 테스트 | **145개 통과** |
| viewer-decoder 단위 테스트 | **24개 통과** |
| Host desktop | **132 lib + 10 e2e 통과**, 기존 2개 ignored |
| control-contract | **14 + 18 통과** |
| Swift split 테스트, library, policy/adaptive/cursor | 통과 |
| 로컬 하드웨어 VT 반복 PTS probe | 통과 |
| Android arm64 release 및 APK release 빌드 | 통과, 기기 설치 성공 |
| macOS 개발 앱 빌드·동일 서명 확인·설치 | 통과, `/Applications/Leftcar Host.app` 실행 |

Swift 테스트는 실제 carrier seed/dequeue/clear 메서드를 실행하지만 전체 callback→스케줄링→종료
통합 경로는 아니다. 새 encoder 생성 probe에는 생성 불가 시 건너뛰는 경로가 있어 실제 재생성
하드웨어 검증 완료로 확대하지 않는다. PTS 초기화 구현과 순수 함수 검증은 확인했다.

최종 APK SHA-256:
`506a9a3f919bfe29a8bd9eb0e614dfb747dcda5c1ccdcb4aa7cdd656c02548b5`

새 Rust 라이브러리와 Gradle 입력이 일치하고, APK의 native 라이브러리는 Gradle strip 결과와 일치했다.
원본 SO와 strip된 SO의 해시가 다른 것은 정상이다.

## 증거 및 남은 범위

현재 머신 임시 영수증: `/tmp/leftcar-latency-baseline/`.
주요 파일: `android-telemetry-baseline-long.log`, `android-telemetry-after.log`,
`android-codec-config-after.log`, `status-timeline.jsonl`, `host-build-install.log`,
`react-doctor-resize-final.log`, `typecheck-resize-final.log`, `vitest-resize-final.log`,
`decoder-tests-final.log`, `apk-final-receipt.json`.

- 관리 가상 화면 resize/배치와 Galaxy XR 비율 프리셋 실기는 미검증이다. Mac 가상 화면 생성·배치·삭제 및 BetterDisplay 재시작은 자동 승인 검토가 거부한 경계로 유지했다.
- 1080p→4K 재구성 시 분할 인코더 자동 복귀는 남아 있다. 새 연결의 4K 자동 선택과 재구성의 기존 모드 유지 정책은 다르다.
- 변경 전 835초 표본의 2초 정지는 Host 출력 정지와 겹쳤지만, 통제되지 않은 화면 변화와 빌드 부하가 있어 carrier 변경으로 원인이 완전히 제거됐다고 단정하지 않는다.
- 외부 전송 승인 범위는 이 프로젝트의 Z.AI 작업이다. 비밀값을 영수증에 출력하지 않았다. 커밋·push·PR은 아직 수행하지 않았다.
