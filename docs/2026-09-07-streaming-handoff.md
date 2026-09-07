# 태블릿 반응성 작업 마무리 및 남은 작업

2026-09-07 사용자 요청에 따라 개발 실행을 중단하고 현재 변경을 보존했다. **전체 개발 완료 또는 배포 가능한 상태라는 의미는 아니다.** 마지막 Android 화면 전환 수정은 컴파일·단위 테스트를 통과했지만 검토 지적과 실기기 검증이 남았다.

## 완료한 변경

- 반응성 우선(기본값)과 화질 우선 설정을 분리하고 저장·이전 설정 마이그레이션을 적용했다.
- 논리 화면 크기와 전송 목표를 분리했다. 16:9는 2560×1440/3840×2160, 지원되는 32:9 소스는 5120×1440을 목표로 계산한다. 세로 화면·짝수 크기·픽셀 예산·작은 소스 확대 금지를 검증했다.
- 자동 적응의 시작 해상도와 최대 목표를 분리했다. 수신 지표의 유효 시간, 실제 0fps, 복구·손실·대기열을 구분한다.
- Host의 인코더 재구성 기능 협상과 실제 수락 모드를 Viewer에 연결했다. 정확한 4K60/UDP 조건 검증 및 구버전 호환을 유지했다.
- Android 디코더/복구 요청 경합, 요청 ID 추적, 프레임 번호 순환 처리를 보완했다.
- Host 캡처·인코더 변경 및 대기시간 전달을 검증했다. 새 복구 요청을 무시할 수 있는 일률적인 PLI 시간 제한은 채택하지 않았다.
- 재생 중 단일/분할 화면 전환을 위한 Kotlin 수정과 회귀 테스트를 추가했다. 이 항목은 아래 P0 지적 때문에 **부분 완료**다.

## 검증과 설치 상태

| 항목 | 확인한 결과 | 한계 |
| --- | --- | --- |
| React | React Doctor 100/100, 루트/Viewer 타입 검사, Vitest 36파일 469개 통과 | 마지막 React 변경 이후 결과 |
| Android Rust | 167+24개 테스트, arm64 release 빌드 통과 | 실기기 입력 지연 보장과 별개 |
| Host/계약 | Host 141개+통합 10개, 계약 15+19개 통과 | Host 기존 테스트 2개 ignored |
| Swift | Split 실행 테스트 통과 | 통제된 네트워크 A/B 아님 |
| 마지막 Kotlin | 실제 Gradle `:app:testReleaseUnitTest --tests dev.leftcar.viewer.stream.StreamSurfaceTransitionTest` 통과: 12개, 실패/오류 0 | 아래 지적을 충분히 검출하지 못하는 테스트가 남음 |
| 1440p 시작 | 약 100초, 2560×1440 유지, Host 수신 FPS 표본 중앙값 54 | 창 전환 포함, 일정 부하 벤치마크 아님 |
| 4K 재생 | 약 180초, 좌/우 평균 59.584/59.635fps, 최대 gap→첫 출력 159.006/156.019ms | Kotlin 전환 수정 **전** 설치 앱의 단기 관측 |
| 1440p 입력 | 명령 시작→녹화 변화 중앙값: 탭 162.0ms, 휠 명령 107.2ms, 드래그 107.7ms | 작은 표본, 탭 1회 미연결, 물리 입력→광자 지연 아님 |
| 4K 입력 | 첫 녹화 정량 결과 제외 | 측정 바코드가 분할 인코더 경계를 가로질러 잘못된 프레임 ID 생성 |

설치된 Host는 새 재구성 기능을 제공한다. 설치 APK SHA-256은 `1773be777ca0de13deacb1910f51bdda929ce5bac76a722cfced796b104c28a9`이며 **최신 Kotlin U3 수정은 설치되지 않았다.** 마지막 Gradle 실행은 컴파일·테스트이며 APK 재설치가 아니다. 종료 시 Host 세션은 0개, Viewer 프로세스는 실행되지 않음을 확인했다.

## 남은 작업 전체

> **2026-09-07 저녁 갱신**: P0-5·P0-7·P0-8 완료. 상세 수치는 `docs/responsive-streaming-validation.md`의 "P0 마무리 실기 검증" 절과 `tools/stream-stats.py`(신규 실시간 통계 도구)를 본다. 아래 목록은 당시 상태 보존용이다.

### P0 — Android 화면 전환을 마무리한 후 새 APK 검증

주 파일: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`, `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/StreamSurfaceTransitionTest.kt`.

1. **이전 holder 콜백의 부수 효과 제거.** 현재 gate가 무시하더라도 Activity의 `surfaceCreated`/`surfaceDestroyed`가 pending attach 취소와 lifecycle 이벤트를 실행한다. 종료된 이전 화면이 새 화면의 붙이기를 취소할 수 있다.
2. **분할→단일 전환의 늦은 첫 destroy 경합 해결.** 옛 왼쪽 destroy → 새 단일 attach → 옛 오른쪽의 첫 destroy 순서는 `stopped` 집합의 중복 방지로 해결되지 않는다. 새 renderer를 detach하지 않는 순서 테스트가 필요하다.
3. **이전 holder 참조 누적 제거.** 현재 `stopped` 집합이 장시간 유지되는 Activity에서 교체한 holder를 계속 보관한다. 명시적 기존 attachment 정리 후 hierarchy 교체와 retired callback 무시처럼 소유권을 단순하게 만드는 방향을 검토한다.
4. **Split 복구의 잘못된 단일 rebind 제거.** `rebindOnSameSurface()`는 여전히 단일 `rebindSurfacePort`를 호출한다. Split local recovery가 단일 renderer로 들어가면 안 된다.
5. **준비된 수신 연결과 새 복구를 구분.** JNI split detach는 suspend가 아닌 stop이므로 수신 소켓이 남지 않는다. 반면 Host 재구성 후 TS가 이미 준비한 소켓을 무조건 `prepareSplitStream`으로 다시 만들면 인증 token까지 잃는다. 준비 완료된 mode-change는 기존 준비를 보존하고, 소켓을 잃은 Split 복구는 React/Host 재준비 경로로 위임하는 방향을 검토한다.
6. **실패/취소 경로 검증.** JNI split attach는 receiver를 먼저 꺼낸 뒤 core attach 실패 시 소실한다. 오른쪽 receiver 누락 시 왼쪽도 소실한다. 단순 attach 재시도만으로 복구된다고 가정하지 말고 전체 재준비/실패 정리를 검증한다.
7. 테스트에 위 순서와 빠른 단일↔분할 왕복을 추가하고 실제 Gradle 테스트를 재실행한다. 현재 12개 통과만으로 P0 해결을 주장하지 않는다.
8. 새 APK를 빌드·설치하고 **같은 Host 세션**에서 1440p→4K→1440p→4K를 검증한다. 각 전환 후 6초 피드백 제한을 넘겨 실제 rendered FPS·해상도·인코더 모드가 유지되는지 확인한다.

실기기에서 이미 재현한 실패: session 3은 1440p→4K 전환을 Host가 수락했으나 split renderer가 붙지 않아 피드백 타임아웃으로 종료됐다. 새 연결이 성공한 것과 같은 연결에서 전환이 성공한 것을 구분해야 한다.

### P1 — 반응성 측정과 성능 한계 확인

- 최종 APK로 녹화 없는 2K/4K 재생을 다시 측정한다. 평균 FPS뿐 아니라 최장 정지, gap→복구, 반복 복구 횟수를 기록한다. 이전 관측의 약 0.928초/2.895초 정지가 모두 해결되었다고 아직 주장할 수 없다.
- 4K 입력은 바코드 전체를 한 인코더 영역 안에 배치해 재측정한다. 임시 `LeftcarBarcodeProbe.app`를 준비했지만 이 재측정은 실행하지 않았다. 24비트 ID·좌표 간격과 녹화 프레임 대응을 다시 검증해야 한다.
- 원격 입력 활성화는 자동 승인 검토가 접근 권한 변경으로 거절했다. 현재 테스트 동안 허용할지 요청했으나 답변은 아직 없다. 응답 전에는 입력을 켜거나 다른 경로로 우회하지 않는다.
- 실제 마우스는 연결되어 있지 않다. 물리 마우스 움직임/클릭/휠과 물리 터치→광자 지연은 미검증이다. 과거 언급한 약 1프레임 반응성을 현재 달성했다는 증거는 없다.
- 5120×1440/32:9 실재생은 실제 지원 소스가 생기면 검증한다. 현재 카탈로그는 3840×2160 한 개다. 계산 통과는 인코더/디코더 지원 증명이 아니다. 이 검증을 위해 가상 디스플레이 생성·변경이나 BetterDisplay 재시작을 하지 않는다.
- 자동 상향/하향을 실제 장시간 연결에서 확인한다. 1440p 시작 관측에서는 상향 전환이 일어나지 않았으며 자동 정책의 모든 실기기 경로를 증명한 것이 아니다.

### P2 — 작업 환경과 통합

- 오른쪽 모니터 열기 요청은 Codex UI에서 `queued`로 반환되었다. 실제 오른쪽 표시/항상 유지까지 확인하지 못했다. 모니터 URL은 `http://127.0.0.1:65527/`이었다. 재개 시 살아 있는 주소와 배치를 확인한다.
- Z.AI 우선 경로를 사용했다. native Loop 호출의 identity 문제로 이번 실행에는 기존 pinned Pi 연결도 사용했다. 정상 Loop 제어 경로와 세션 연결 문제를 별도 확인해야 한다. 조용하다는 이유로 다른 provider로 전환하지 않는다.
- 후속 Z.AI 작업 `01a07a64-19c5-7493-8dea-9832a256be32`는 사용자 마무리 요청으로 `stopped`를 확인했다. 해당 후속 작업은 read 3회만 했고 추가 소스 수정은 없었다. 앞선 U3 결과와 모든 기존 변경은 보존했다.
- 아직 커밋·push·PR은 하지 않았다. P0 정리 후 변경 파일을 다시 검토하고 커밋별 확인을 받아야 한다. `.loop/` 실행 자료와 빌드 산출물을 소스 커밋에 섞지 않는다.
- 서명·배포·스토어 작업은 이번 완료 범위에 포함하지 않는다. `docs/tablet-cursor-streaming-validation.md`는 수정하지 않았다.

## 재개 순서와 증거

1. 이 문서의 P0부터 처리하고 기존 변경을 보존한다. 구현·독립 검토는 Z.AI 우선, 기기/Host 작업은 한 담당자가 직렬 실행한다.
2. Kotlin 테스트 후 APK 빌드. Rust를 다시 빌드했다면 `target/aarch64-linux-android/release/libleftcar_viewer.so`를 Gradle 입력 `apps/viewer-expo/android/app/libs/arm64-v8a/libleftcar_viewer.so`에 반영해야 한다. 이번에 Gradle이 자동 복사하지 않는 것을 확인했다. APK 안의 라이브러리와 strip 산출물의 해시도 비교한다.
3. 실제 전환 검증 시 MainActivity를 **스트림 시작 전** freeform으로 연다. 두 창을 보이게 유지한다. 재생 중 전체 화면→freeform 변경은 Surface 자체를 파괴할 수 있어 전환 검증과 혼동하면 안 된다. 새 Task ID와 버튼 위치를 매번 확인한다.
4. React를 수정하면 저장소 루트에서 React Doctor 100/100을 충족한 뒤 타입 검사와 관련 테스트를 다시 실행한다.
5. 결과와 설치 APK 해시를 `docs/responsive-streaming-validation.md`에 갱신하고 커밋 계획을 확정한다.

상세 수치: `docs/responsive-streaming-validation.md`. 구현 계획: `docs/plans/2026-09-07-responsive-streaming.md`.

주요 테스트 로그·측정 요약·실패 로그는 저장소의 **Git 제외** 디렉터리 `artifacts/responsive-streaming-2026-09-07/`에 복사했다. `manifest.json`에 복사된 파일의 해시와 크기가 있다. 원본 영상/도구 전체는 `/tmp/leftcar-responsive-validation/`에 남아 있으며 임시 경로이므로 영구 보존을 보장하지 않는다. 후속 작업 중단 증거는 `/tmp/leftcar-loop-pinned-20260907/u3-followup-stop.jsonl`이다.

커밋 계획 초안은 `artifacts/responsive-streaming-2026-09-07/commit-plan.md`에 있다. 현재 6개 묶음: Viewer 네이티브 성능, Host 네이티브 성능, 제어 계약, Android 화면 전환, Viewer 설정/정책, 검증 문서. 이 인계 문서도 마지막 문서 커밋에 포함한다. 생성 승인은 아직 받지 않았다.
