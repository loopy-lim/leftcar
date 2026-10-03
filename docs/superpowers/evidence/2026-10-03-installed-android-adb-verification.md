# 기존 Android 설치본 ADB 실행 검증

2026-10-03 KST, 사용자의 Android 앱 ADB 확인 요청에 따라 TB710FU의 기존 `leftcar.ll3.kr`을 실행했다. 기존 앱으로 진행하라는 앞선 요청을 유지했다. 설치·업데이트·데이터 초기화·OS 권한 허용·설정 토글·재페어링·파일 전송·가상 디스플레이 변경은 수행하지 않았다.

## 검증 대상과 경계

- 기기: TB710FU, Android 16 / SDK 36
- ADB transport: LAN 기기의 기존 ADB endpoint, 상태 `device`
- 설치 패키지: `leftcar.ll3.kr`, versionName **0.1.10**, versionCode **16**
- 설치 APK SHA256: `53dad236d7b1ba4c2f30c38b706281cec42d99abf729357dcdc5bf372540cfdd`
- 마지막 업데이트: `2026-09-22 14:51:11`
- 실행 Activity: `dev.leftcar.viewer.MainActivity`, cold start `Status: ok`, TotalTime `214 ms`
- 검증 PID: `16598`
- 실제 앱 configuration: 가로 3200×2000 px, 400 dpi, 1280×800 dp

이 APK의 소스 provenance는 현재 작업 트리와 대조되지 않았다. 표시 버전만으로 최신 안전성·디자인 수정이 포함됐다고 판단하지 않는다. [최신 작업 트리 자동 검증](2026-10-03-safety-and-ux-verification.md)과 이 기존 설치본의 실제 실행 결과는 별개다.

## 실제 동작 확인

| 항목 | 결과와 근거 |
|---|---|
| 앱 시작과 기존 연결 | 실행 전 Viewer PID가 없었고 초기 화면 확인에서도 StreamActivity는 없었다. 정상 시작 뒤 저장된 LAN Host의 화면 선택 화면이 표시됐다. Mac의 기존 listener가 켜진 것도 읽기 전용으로 확인했다. |
| 기본 화면 | 앱 전면 상태에서 한 번의 기기 캡처와 UI hierarchy로 화면 목록·품질·설정·Display 0을 확인했다. Mac 영상 화면은 캡처하지 않았다. |
| 설정 표시와 Back | 상단 설정 버튼으로 시트를 열어 switch 상태를 읽었다. Back으로 닫힌 뒤 화면 선택 화면이 다시 표시됐다. 설정 토글이나 입력은 하지 않았다. |
| 기존 주 화면 열기 | 기존 Display 0만 열었다. 새 native SurfaceView가 `[0,100][3200,1900]` 영역에 생성됐다. `AndroidDecoder created successfully: codec=Hevc actualCodec=c2.qti.hevc.decoder.low_latency`를 확인했다. 원격 입력이나 시스템 소리 설정은 변경하지 않았다. |
| 디코드 출력 진행 | 동일한 PID/stream/incarnation/decoderEpoch에서 Surface 제출 횟수가 증가했다. 아래 관찰 구간과 최종 카운터를 따른다. |
| 새 영상 창 닫기 | Back으로 “이 화면을 닫을까요?” 확인 창이 표시됐다. 검증에서 새로 연 창의 “화면 닫기”를 눌렀고, 화면 선택으로 돌아왔다. Native renderer 종료와 JS cleanup 완료까지 확인했다. |
| 앱 Back 복귀 | 화면 선택 → Leftcar 홈 → 원래 전면 앱으로 돌아왔다. 최종 상태에 StreamActivity가 없고 앱 프로세스 PID 16598은 남았다. 강제 종료나 task 제거는 하지 않았다. |
| 설정·설치·권한 유지 | 보이는 네 가지 switch 값이 영상 확인 전후 같았다. APK SHA256, versionName, lastUpdateTime, `dumpsys package`의 설치·runtime 권한 13개 허용 상태가 전후 같았다. 기존 CAMERA 허용도 그대로였고 카메라는 실행하지 않았다. |

### 영상 관찰 수치

동일한 `process=16598`, `stream=5001`, `incarnation=16598-1790959633513510391-1`, `decoderEpoch=0`, `outputStage=surface-release`의 canonical 표본 90개를 수집했다.

- 처음: epoch `1790959634.372`, `released=30`
- 마지막: epoch `1790959678.922`, `released=2700`
- 관찰 간격: **44.55초**, 증가량 **2670**, 이 구간의 Surface 제출율 약 **59.93회/초**
- 종료 전 마지막 canonical/legacy 기록: `released=5820`, `decoderInputsQueued=5820`, `decoderInputDrops=0`, `frameGaps=0`

이 값은 디코딩된 출력의 Surface 제출이다. 물리 패널 FPS·화면 내용의 시각적 정확성·광학 지연·장시간 안정성·4K60 수용성으로 해석하지 않는다. 앱의 화면 목록에 표시된 2560×1440/60 FPS와 실제 인코더 출력 설정의 동일성도 별도로 대조하지 않았다.

### 종료 완료 근거

새로 연 port `5001`에 대해 다음 로그를 관찰했다.

```text
window close confirmed port=5001
window close event port=5001 generation=8261511824660 current=8261511824660
surfaceDestroyed: stop=FINAL_RELEASE generation=3 geometryChanges=1 instanceId=src-5001
onDestroy: final release instanceId=src-5001 attached=false
Sent stream close signal for instance src-5001
Live stream renderer exiting for instance
[leftcar] window cleanup complete session=1
```

`onDestroy`의 release 요청만으로 완료라고 판단하지 않았다. 후속 renderer exit와 native close ACK를 기다린 JS cleanup 완료, SurfaceView가 사라진 실제 UI를 함께 확인했다. Surface generation과 창 ownership generation은 서로 다른 값이다. 종료 시 기존 ReleaseAll/BYE 제어 동작은 자동으로 발생할 수 있으며, 수동 원격 입력을 주입하지 않았다는 범위로 기록한다.

## 사용자 상태와 UI 관찰

실제 설정은 FPS 표시 켬, 커서 오버레이·시스템 소리·클립보드 공유 끔이었다. 이 네 값은 종료 후 설정 시트를 다시 열어 같은 값으로 표시되는 것을 확인했다. 원격 입력은 기존 “허용됨” 표시를 읽었으며 승인·철회·입력 조작을 하지 않았다.

상단 설정 버튼의 접근성 bounds는 `[3070,142][3160,214]`, 즉 **90×72 px**였다. 실제 앱 resource configuration의 400 dpi로 환산하면 **36×28.8 dp**로, 요구한 44dp 영역보다 작게 노출된다. 시트 닫기 버튼은 130×130 px, 52×52 dp였다. 상단 버튼의 실제 `hitSlop`이나 TalkBack 터치 탐색 범위는 측정하지 않았으므로 접근성 bounds와 실제 전체 터치 영역을 동일하다고 단정하지 않는다. 이 설치본의 추가 확인이 필요한 UI 개선점이다.

동일 설정을 여는 본문 버튼도 별도로 있으며 74.8×45.2 dp로 노출됐다. 네 가지 switch의 접근성 bounds는 각각 47.2×27.2 dp였다. 작은 상단 버튼과 switch의 실제 조작 영역, 중복 설정 진입점은 후속 UI 점검 대상으로 남겼다. 독립 검토가 로컬 기록의 표본·종료·권한·bounds 계산을 재확인했으며 기기를 추가 조작하지 않았다.

관찰 중 UI 조작에 응답했고, 수집한 PID 로그에서 `FATAL EXCEPTION`, fatal signal, JS error와 ANR marker는 발견되지 않았다. 이것은 이번 짧은 실행과 해당 로그의 범위이며 모든 시스템 ANR·장기 안정성 검증이 아니다.

`run-as`가 허용되지 않는 일반 설치본이므로 private app data와 SecureStore 전체를 읽거나 해시 비교하지 않았다. 정상 실행·자동 재연결은 최근 연결 metadata 등을 갱신할 수 있다. 패키지의 `stopped` 표시는 실행 전 true에서 실행 후 false로 바뀌었고 앱 프로세스도 남았다. APK·보이는 설정·권한 유지와 앱의 전체 실행 상태를 구분한다. 데이터 초기화를 하지 않았다는 사실을 앱 내부 파일이 전혀 쓰이지 않았다는 뜻으로 확대하지 않는다. 기본 화면·설정·현재 Host 연결·단일 영상·Back·새 창 정리까지만 확인했으며, QR/카메라·권한 요청·파일·오디오·원격 입력·USB·여러 영상 창·재연결 장애 복구는 이번 검증에 포함하지 않았다.

## 보조 기록

원본과 요약을 `/private/tmp/leftcar-android-installed-runtime-20261003-tpi2wboc/`에 보관했다.

- `preflight.json`, `package-before.txt`, `package-after.txt`, `package-comparison.json`
- `permission-full-comparison.json`은 설치와 runtime 권한을 함께 대조한 최종 기록이다. 초기 `package-comparison.json`의 12개는 runtime CAMERA를 제외한 집계였다.
- `apk-before.json`, `apk-after.json`, `launch.txt`, `initial-health.json`
- `catalog.png`, 화면·설정·종료 확인 UI hierarchy XML
- `settings-comparison.json`, `display-configuration.json`, `final-navigation.json`
- `stream-surface-samples.json`, `stream-perf-sample1.json`, `stream-close-result.json`, `final-native-evidence.json`

전체 logcat이나 자격 증명·클립보드 내용은 출력·저장하지 않았다. 필요한 native lifecycle 메시지와 whitelist 수치만 기록했다. 이번 작업에서는 React·Kotlin·Rust 구현 소스를 변경하지 않았다.
