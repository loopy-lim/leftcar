# Sidecar 기준 확장 화면 점검·검증

## 사용자에게 보이는 변화

- 태블릿에서 기존 확장 화면을 바로 다시 열 수 있다. 스트림 종료나 네트워크 단절은 Mac 작업 공간을 삭제하지 않는다.
- Mac과 태블릿에서 왼쪽·오른쪽·위·아래 배치를 선택할 수 있다. 배치 후 커서 및 원격 클릭 좌표가 현재 화면을 따른다.
- 자동 패널 크기, 프리셋, 직접 크기, Retina 배율을 명시적으로 적용한다. 전송 해상도와 Mac 작업 공간 크기를 구분한다.
- 제거 완료는 실제 macOS 활성 디스플레이 목록을 기준으로 판단한다. 크기 변경 시 사용하는 스트림을 먼저 닫고 재생성한 화면을 다시 연다. 다른 기기가 사용 중이면 변경을 거절한다.
- 느린 변경 뒤 상태 조회가 기다리는 시간을 조회 timeout에서 제외한다. 다른 연결의 화면 변경과 충돌하면 연결을 끊지 않고 busy 응답을 보낸다.

## 전체 기능 점검

비교 출처: [Apple Sidecar](https://support.apple.com/en-au/102597), [iPad 두 번째 디스플레이 안내](https://support.apple.com/guide/ipad/use-your-ipad-as-a-second-display-ipad2b1aa3be/ipados).

| 기능 | Leftcar 확인 결과 | 이번 처리 / 남은 차이 |
|---|---|---|
| 확장/기존 화면 보기 | 단일 native 가상 화면, 기존 실제 화면 공유 | 기존 확장 화면 열기를 주 동작으로 변경; 복제는 기존 실제 화면 선택 |
| 기기 선택·재연결 | 저장된 Host, LAN/외부 경로, selection generation | 늦은 생성 응답이 다른 Host를 열지 않도록 보호; 기존 UUID 재사용 |
| 종료·다시 열기 | Mac이 작업 공간 수명을 소유 | 단순 창 닫기와 화면 제거를 명확하게 분리; 실제 제거 소실 확인 |
| 배치·Mac 마우스 이동 | CoreGraphics 디스플레이 좌표 | 네 방향 배치 제공; 화면 간 간격 및 primary 변경 없는지 실기 확인 |
| 해상도·글자 크기 | 논리 크기와 backing 픽셀, 패널 메트릭 | 프리셋 밖 16:9 자동 크기 수정; 직접 크기/Retina 적용 |
| 원격 포인터·드래그 | 입력 허용, 버튼 매핑, release-all | 이동한 디스플레이의 실시간 bounds와 마지막 픽셀 경계 수정 |
| 키보드·한영·단축키 | 물리 키보드, 입력 언어 전환, 텍스트 relay | 기존 Swift/Rust/Android 회귀 검사 대상; 별도 OS 권한은 유지 |
| 터치 | 탭, 길게 누르기, 드래그, 두 손가락 스크롤/줌 | 기존 Android 제스처와 안내 유지, 회귀 검사 대상 |
| 펜 | 압력 suffix와 CGEvent tablet pressure 경로 | Apple Pencil 전용 API·hover·double tap과 동일하다고 주장하지 않음 |
| 보조 UI | 스트림 메뉴, 소프트 키보드, 입력/품질 설정 | 배치·크기 설정 추가. Sidecar Touch Bar·전용 sidebar 복제는 제공하지 않음 |
| 앱 전환·회전·복귀 | Android window/connection/recovery 상태 머신 | 기존 테스트로 점검. 현재 후보 실기 장시간/잠자기 복귀는 미검증 |
| 클립보드·파일 | 기존 승인된 공유 기능 | 권한·기기별 경계 유지. Apple Continuity와 동일한 시스템 통합 아님 |
| 유선·무선 | 기존 USB/LAN/외부 전송 경로 | 전송 알고리즘 변경 없음; 이번 실행에서 태블릿 LAN/외부 장시간 재검증하지 않음 |
| 시스템 통합 | 앱이 가상 화면을 관리 | Control Center/AirPlay/Apple 계정 연속성 통합은 OS 전용 기능 |

## 원인과 수정 근거

Objective-C 런타임의 init 메서드를 일반 함수 포인터로 호출해 ARC가 소유권을 잘못 처리했다. registry에서 제거해도 객체가 살아남았다. init family 메서드 선언과 정상 ObjC 호출로 고친 뒤 동일 프로세스에서 생성·제거가 통과했다.

HiDPI 화면의 첫 배치에서 WindowServer가 이전 크기로 origin을 정규화해 간격을 만드는 사례를 확인했다. 배치 트랜잭션에는 **논리 크기와 backing 픽셀이 현재 선언과 일치하는 같은 모드**만 함께 지정한다. 재생성 직후 생성 프로세스의 모드 조회가 null인 사례는 실기 회귀에서 재현했으며, 이때는 origin만 적용한다. 위치가 첫 트랜잭션 뒤 맞지 않으면 완료를 기다린 다음 한 번만 재시도한다. 크기 자체는 제거·재생성하며 다른 모드를 고르지 않는다. 이는 기존 E-8의 blanket 금지를 현재 증거에 따라 좁힌 결정이다.

화면 변경과 스트림 시작은 reader/writer gate로 보호한다. 서로 다른 스트림 시작은 병행되며 기존 supersession을 유지한다. 다른 연결에서 변경 중인 화면을 조회·재구성할 때는 기다리다 소켓이 끊어지지 않도록 즉시 busy 응답을 반환한다.

## 검증 기록

- React Doctor: 저장소 전체 **100 / 100**, ignore/override 없음.
- TypeScript: 전체 typecheck 통과. Vitest 70개 파일 **852개** 통과. 계약 JS 검사 4개 통과.
- Host Rust: 최종 전체 283개 + control E2E 13개 통과. cross-connection busy 회귀 포함.
- Rust control-contract: 단위 14개 + 계약 20개 통과. 기존 JSON 호환 필드는 optional/default.
- 실제 Host React 카드 + 실제 Viewer hook/session 브라우저 회귀: 5개 통과. 프리셋 밖 자동 크기, 선택만으로 생성하지 않음, pending 유지/해제, Host 전환 후 busy/늦은 응답 취소.
- 실제 FIFO control client: 느린 resize 뒤 두 상태 조회가 12초 대기하면 기존 코드에서 socket destroy 2회 재현(RED). 수정 후 연결 유지 및 앞 순서 도달 뒤 진짜 timeout 모두 통과(GREEN).
- Swift cursor: 배치 이동 후 커서 위치 갱신 및 포인터 끝점 범위 검사 통과. 마우스 버튼 매핑, 입력 언어 전환 검사 통과.
- **실제 Mac 디스플레이 검증**: 1280×800 @2, 1440×900 @2, 1600×1000 @1 각각 생성→네 방향 배치→제거 통과. 기본 화면 ID/위치 유지, 화면 간 틈 없음, 제거 후 활성 목록에 남지 않음. 모두 동일 source UUID 확인.
- 최종 native 실기: 이전 크기에서 재생성 후 왼쪽부터 배치, 동일 1280×800의 1×→2× 및 2×→1× 전환 후 네 방향 배치·제거 통과. 매 배치마다 별도 프로세스로 실제 논리 크기와 backing 픽셀을 검사했다. 주 화면 ID·위치 유지.
- Android: `:app:testReleaseUnitTest --rerun -PleftcarInternalBuild=true` 실행. 26개 suite, 145개 검사, 실패·오류·생략 0. 캐시 재사용이 아닌 테스트 실행 완료.
- Mac 소유 프로세스 비정상 종료: 시험용 프로세스만 강제 종료 후 가상 화면 자동 회수, 기존 화면 목록/배치 동일 확인.
- 독립 코드 검토에서 지연 제거, stale pending, Host 전환 busy, FIFO timeout, 다른 연결 잠금 대기를 지적받아 수정하고 회귀 검사했다.

자동 검사와 실기 증거는 다르다. Android 설치 후보 빌드는 실제 태블릿 영상·키보드·터치의 종단간 확인을 대신하지 않는다. 기존 서명과 일치하는 앱 업데이트를 수행했으며 데이터 초기화나 권한 추가는 하지 않았다. 업데이트 직후 Host 식별 정보·페어링·source grants·설정의 해시가 모두 기존과 같음을 확인했다. Sidecar와 동등한 지연·화질·장시간 안정성을 측정했다고 주장하지 않는다.

실행 로그는 `/tmp/leftcar-sidecar-*.log`에 있다. 새 확장 UI 검사는 `bun tools/ui-regression/build.mjs --extended-display && bun tools/ui-regression/extended-display.cjs`로 실행했다. 기존 전체 UI harness는 이전에 삭제된 SourceGrantEditor를 가져오는 별도 fixture 때문에 빌드되지 않으므로 전체 UI suite 통과로 표시하지 않는다. 최종 패키지 경로·해시는 빌드가 발행하는 manifest가 기준이며, 코드 변경 중 만들어진 중간 후보는 source-changed 검증으로 거절했다.

## 실기 인수 순서

1. 새 Host/Viewer 후보를 동일 조합으로 사용하고 기존 페어링·입력 권한을 확인한다.
2. 태블릿에서 확장 화면 만들기 → 창 이동 → 스트림 닫기 → 기존 확장 화면 열기.
3. 네 방향 배치에서 Mac 마우스 경계 이동과 태블릿 클릭/드래그 위치를 확인한다.
4. 자동/직접 크기를 적용하고 글자 선명도, 스트림 재열기, 다른 기기 사용 시 거절을 확인한다.
5. 키보드 한영·단축키·터치 스크롤/줌·펜·앱 전환/잠자기 복귀를 확인한다.
6. LAN/외부 각각 충분한 사용 시간 동안 영상 연속성과 지연을 별도로 측정한다.

## 최종 설치 후보

두 패키지의 빌드 입력 해시는 동일하다: `6690ee7c1a1a108dd1fa5919c8b8535a75abbb0f616cbe9235fb6b89aa146196`. 빌드 후 수정된 검증 문서는 빌드 입력에서 제외된다. 버전은 0.1.10, Android versionCode 16이며 내부 검증용 후보이다.

- [Mac Host 앱](</var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/leftcar-host-macos-internal-BdFSOS/artifacts/host-bundle-Leftcar Host.app>) — SHA-256 `09a4455c8b3ad42d6426d222f8f84e9b3c6467bd06c1486d24ec317958e9dace`. 설치된 앱과 동일 Apple Development 인증서 서명. 기존 앱의 designated requirement 검증 및 fresh shim 일치 확인, notarization 없음.
- [Android APK](/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/leftcar-android-internal-yAEsUX/artifacts/viewer-apk-app-release.apk) — SHA-256 `53dad236d7b1ba4c2f30c38b706281cec42d99abf729357dcdc5bf372540cfdd`. 내부 서명, APK 내 native/JS 바이트 확인.
- [Host 빌드 기록](/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/leftcar-host-macos-internal-BdFSOS/host-macos-internal-manifest.json)
- [Android 빌드 기록](/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/leftcar-android-internal-yAEsUX/android-internal-manifest.json)

## 설치 및 앱 실기 기록

- 최종 패키지 두 개의 source/artifact manifest 검증 통과. Android는 설치본과 최종 APK SHA-256이 같아 재설치하지 않았으며, Host는 마지막 native 수정본으로 재교체했다.
- 사용자 요청으로 `/Applications/Leftcar Host.app`과 TB710FU의 `leftcar.ll3.kr`을 업데이트했다. 기존 Host 앱은 `/tmp/leftcar-sidecar-device-run-20260922/Original Leftcar Host.app`에 보존했다. Android는 update-only 설치이며 기존 데이터와 페어링을 초기화하지 않았다.
- Host 재실행에서 입력 권한 승인 유지와 새 확장 화면 UI를 확인했다. Host 식별 정보·페어링·source grants·설정은 업데이트 직후 바이트 해시가 모두 동일했다. 기록: `/tmp/leftcar-sidecar-device-run-20260922/state-before.json`, `state-after-update.json`.
- 실제 Host UI로 확장 화면 생성·배치·제거를 실행했다. 첫 왼쪽 배치에서 `-1600,0 1280x800` 간격과 오류를 관측했고, 재시도에서 `-1280,0`으로 맞았다. 이를 계기로 이전 크기에서 재생성한 뒤 첫 배치를 검사하도록 실기 probe를 보강했다.
- 보강 전 probe는 재생성 직후 `CGDisplayCopyDisplayMode=null`로 배치 rc=2를 재현했다. null/오래된 크기·배율에서는 모드 재적용을 건너뛰고, 위치 불일치에만 한 번 재시도하도록 수정했다. 로그: `/tmp/leftcar-sidecar-first-placement-red.log`, `first-placement-debug.log`, `first-placement-final.log`.
- 최종 설치 Host UI에서 첫 왼쪽 배치 `-1280,0 1280x800`, 1440×900 크기 변경 뒤 위 `0,-900`, 아래 `0,1080`, 오른쪽 `1920,0`을 외부 프로세스로 확인했다. Retina backing은 각각 2560×1600 / 2880×1800으로 유지됐다. 제거 뒤 기본 화면 ID 3, `0,0 1920x1080`만 남았다. 기록: `/tmp/leftcar-sidecar-device-run-20260922/final-host-*.txt`.
- 최종 설치 Host의 번들 해시는 위 manifest와 일치한다. 마지막 업데이트 뒤 Host 식별 정보·페어링·설정 해시도 그대로다. `source_grants.json`은 승인된 기기에 새 가상 화면을 추가하는 생성 동작으로 갱신됐다. 최초 업데이트 직후 바이트 보존과 화면 생성 후 권한 목록 갱신을 구분한다.
- 처음 태블릿 확인에서는 조작 도구가 입력을 전달하지 못했고 사용자 화면 보고와 연결 상태가 일치하지 않았다. 아래 재검증에서 같은 USB TB710FU의 실제 StreamActivity, 화면 이미지, Mac 시험 문서 입력, 영상 로그를 함께 확인해 이 불일치를 해소했다.
- 사용자가 로컬 커밋을 명시적으로 승인했다. push·PR·공개 배포는 이번 범위에 포함하지 않는다.

## 태블릿 재검증 — 2026-09-22

사용자가 ADB 직접 조작을 명시적으로 허용한 뒤 TB710FU(Android 16, `HA2D6EMP`)의 설치된 최종 후보를 검증했다. 재설치·데이터 초기화·페어링 변경은 하지 않았다. 태블릿 카탈로그의 **USB 연결됨 · USB 우선**을 확인했으므로 아래 결과는 USB 연결의 결과이다. Mac 자동 검사와 이미 통과한 배치 전체 조합은 반복하지 않았다.

| 항목 | 실제 관측 | 판정 |
|---|---|---|
| 확장 화면 생성·영상 | 태블릿에서 생성 후 StreamActivity가 열리고 Mac 확장 화면 영상 및 디코더 프레임 증가 확인 | 통과 |
| 스트림 닫기·다시 열기 | 뒤로 가기 확인으로 닫은 뒤 Mac display ID 84 유지, `확장 화면 열기`로 같은 화면 재사용 | 통과 |
| 클릭·드래그 | 시험용 TextEdit 문서의 지정 위치에 입력, 첫 줄 드래그 선택 후 `DRAG_OK`로 교체됨 | ADB 입력 경로 통과 |
| 한영 전환·문자 입력 | `KEYCODE_LANGUAGE_SWITCH` 후 `QA_EN_0922`, 다시 전환 후 `rksk`가 `가나`로 조합됨 | ADB 입력 경로 통과 |
| Command+A | ADB Meta+A 두 방식 모두 전체 선택이 되지 않아 후속 시험 문자열이 기존 문장에 추가됨 | 실패 관측, 원인 미확인; 실제 키보드 결과로 일반화하지 않음 |
| 앱 전환 | Android 홈으로 이동한 뒤 돌아와 기존 스트림이 카탈로그에 유지됨 | 짧은 앱 전환 확인 |
| 사용 중 크기 변경 | 1600×1000 @2 → 1280×800 @2 확인 후 기존 스트림 종료·새 StreamActivity 자동 열기, backing 2560×1600 확인 | 통과 |
| 원래 크기 복원 | 1600×1000 @2로 복원 후 자동 재열기, backing 3200×2000 및 최종 왼쪽 `-1600,0` 확인 | 통과 |

크기 변경 중 1280×800 화면은 오른쪽 `1920,0`에 생성됐고, 원래 크기로 복원한 최종 화면은 왼쪽 `-1600,0`이었다. 따라서 크기 변경 전후 배치·창 위치 보존까지 통과했다고 주장하지 않는다. 기본 화면 ID 3과 `0,0 1920×1080`은 유지됐다.

첫 연속 스트림의 로그 구간은 728.43초이며 Surface-release 누적 프레임 30→42630, 평균 58.48회/초였다. 1280×800 변경 후 282.10초 구간은 54.77회/초, 원래 크기 복원 후 35.07초/128.22초 구간은 59.88/56.16회/초였다. 수집 로그의 `outputDrops`, `staleInputDrops`, `unrecoveredFecGroups` 최댓값은 0, `decoderInputDrops`는 2였다. 마지막 구간에는 `frameGaps=4`, `recoverySkippedFrames=4`, `nacksSent=6`도 관측했으며 이후에도 영상 프레임은 증가했다. 이는 디코더가 Surface로 전달한 주기와 해당 카운터의 관측값이며, 실제 패널 표시 FPS·종단간 지연·장시간 무손실 보장은 아니다. 60fps 고정이나 무손실 통과로 표시하지 않는다.

시험 문서는 저장 후 닫고 조작용 미러 창을 종료했다. 최종 태블릿은 원래 크기의 확장 화면을 열어 둔 상태다. 입력 언어는 시험 전 한글 상태로 되돌렸다. 상세 기록은 `/tmp/leftcar-sidecar-device-run-20260922/`의 `tablet-stream-final-summary.json`, `tablet-stream-final.log`, `tablet-display-final.txt`, `tablet-display-after-*.txt`, `tablet-*.xml`, `Leftcar Tablet Input QA.txt`에 있다.

**남은 검증 경계:** 실제 손가락 다중 터치·스크롤/줌, 펜, 물리 키보드 조합키, 잠자기 복귀, LAN/외부 장시간 사용은 이번 ADB 재검증으로 확인하지 않았다. Command+A 원인은 ADB 주입, Android 입력 처리, 앱 전달 경로 중 어디인지 분리되지 않았다. 따라서 기본 확장 화면 사용 흐름은 확인했지만 모든 입력 기능이나 Sidecar 수준의 사용성을 최종 인수했다고 표시하지 않는다.
