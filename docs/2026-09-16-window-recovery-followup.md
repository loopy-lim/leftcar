# 창 독립성 수정과 1440p 성능 재측정

측정일: 2026-09-16. 앞선 [검증 보고서](2026-09-16-final-test-followup.md)의 후속 작업이다. 앞선 설치본의 결과를 이번 설치본의 결과로 합산하지 않는다.

## 결론

**창 독립성 실기 검사와 자동 검사는 통과했지만 1440p60 성능은 아직 미합격이다.** 메인 화면이 사라진 뒤에도 영상 창의 복구·종료 처리를 유지하도록 수정했다. 창 수명 실기 결과와 설치본 식별은 아래 증거 및 `completion-receipt.json`에 기록한다.

원본 자료: [`artifacts/lifecycle-repair-2026-09-16/`](../artifacts/lifecycle-repair-2026-09-16/).

## 수정한 동작

- 영상 복구 이벤트 구독을 화면 목록 컴포넌트에서 지속되는 세션 저장소로 옮겼다. 메인 창이 사라져도 복구에 필요한 객체와 요청 경로가 남는다.
- 제어 소켓의 예기치 않은 단절 때 검증된 Host 선택을 보존한다. 명시적인 연결 해제나 다른 Host 선택은 계속 이전 요청을 취소한다.
- 복구 시 이미 사라진 화면 목록의 조회 객체 대신, 선택된 Host에 직접 소스 목록을 요청한다.
- 사용자가 영상 창을 닫으면 논리 세션을 먼저 제거한다. 늦게 끝난 재연결은 새 창을 되살리지 않고 자기 결과를 정리한다.
- 네이티브 창의 세대 번호로 오래된 종료 알림을 거른다. 이벤트를 큐에 넣기 전에 번호를 확보해 전달 도중 소유권이 바뀌는 경우도 구분한다.
- 재연결로 새로 만든 공유 세션의 종료 경로도 함께 갱신한다. 이전 선택에 묶인 종료 요청이 취소되어 Mac의 공유만 남는 경우를 회귀 테스트로 재현했다.

## 실기 확인 범위

Lenovo TB710FU, Android 16, 같은 LAN의 Tailscale 주소와 UDP로 시험했다. 별도 측정 앱이나 화면 미러링을 실행하지 않았다. 클릭 위치·표시 복귀 확인에만 화면을 읽고, 성능은 상태 JSON과 네이티브 로그로 수집했다.

- 중간 설치본에서 메인 창 제거 후 약 105초 뒤 같은 영상 창으로 복귀해 4K 공유가 재개되는 것을 확인했다(`v3-main-absent.txt`, `v3-return.jsonl`, `v3-events.log`). 기존 Host 세션이 끝난 뒤 새 세션으로 복구된 결과이며, 백그라운드에서도 전송이 끊기지 않았다는 뜻은 아니다.
- 같은 중간 설치본에서 영상 창을 닫아도 Host가 feedback timeout으로 종료되는 실패를 확인했다. 종료 알림·논리 세션 제거·네이티브 정리 성공만으로 Host 종료를 판정하면 안 된다(`diag4k-close-events.log`, `diag4k-close.jsonl`).
- 최종 수정본의 메인 제거·백그라운드 복귀·닫기 기록은 `replacement-*` 파일로 분리했다. 상세 판정은 후술한다.

### 최종 설치본의 창 수명 검사: PASS

1. 4K split 공유 세션 22, 영상 task 622를 시작했다.
2. 메인 task 621만 제거했다. 영상 창은 전경에 남았고, 이후 8개 상태 표본 모두 같은 공유가 실행 중이었다(`replacement-main-removed.txt`, `replacement-main-removed.jsonl`).
3. 홈으로 이동했다가 약 **137.7초** 뒤 같은 영상 task 622로 복귀했다. MainActivity가 없는 상태에서 새 공유 세션 23이 만들어졌고, Surface 재부착은 복귀 후 약 **1.82초**에 완료됐다. 영상 표시도 확인했다(`replacement-return-activities.txt`, `replacement-return.jsonl`, `replacement-close-dialog.png`).
4. 화면 닫기 확인은 23:23:10.053, 첫 Host 세션 제거 관측은 23:23:13.007로 약 **2.95초**였다. feedback timeout이 되기 전에 명시적 종료로 사라졌다. 이후 9개 상태 표본은 빈 세션 목록이었다(`replacement-events.log`, `replacement-close.jsonl`).
5. 메인 창과 목록을 다시 열었다. 영상 창이 부활하지 않았고 추가 8개 상태 표본도 빈 세션 목록이었다(`replacement-after-main-reopen.txt`, `replacement-after-main-reopen.jsonl`).

이 검사는 한 번의 최종 시나리오에 대한 기능 합격이다. 강제 프로세스 종료나 Android가 앱 전체를 제거한 상황까지 영상 유지가 보장된다는 뜻은 아니다. 4K 고동작 성능 합격에도 사용하지 않는다.

## 1440p60 고동작 비교

같은 고동작 장면을 약 60fps로 계속 표시한 상태에서 각 설정을 181초씩 측정했다. 태블릿 전경과 원본 표시·포커스·전체 화면·움직임 검사는 두 구간 모두 통과했다. 실제 압축은 HEVC이며 `rateControl` 상태 문자열만으로 압축 형식을 판별하지 않았다.

| 관측 | UDP 자동 | 묶음 16, FEC 2, 자동 조절 끔 |
| --- | ---: | ---: |
| 유효 측정 | 181.09초 | 181.12초 |
| RTT 표본 p50 / p95 | 27 / 50ms | 29 / 57ms |
| 렌더 FPS 표본 p50 / 하위 5% | 58 / 38 | 55 / 1 |
| 프레임 gap 증가 | 27 | 27 |
| 불완전 압축 프레임 증가 | 233 | 182 |
| 복구 키프레임 증가 | 29 | 87 |
| 중복 복구 요청 억제 증가 | 81 | 2816 |
| 송신 실패 / 비트레이트 하한 압박 | 0 / 0 | 0 / 0 |
| 캡처→소프트웨어 출력 나이 표본 p50 / p95 | 42 / 98ms | 37 / 78ms |
| 종합 판정 | **FAIL** | **FAIL** |

RTT p95 20ms 이하, 프레임 gap·복구 없는 출력 기준을 충족하지 못했다. 출력이 멈출 때 로그도 비는 구간이 있으므로 전체 구간 출력 FPS 및 정식 출력 지연 판정은 **UNKNOWN**이다. 표의 출력 나이는 주기적 로그의 표본 분포이며 실제 패널 표시 지연이나 물리 입력→화면 지연이 아니다.

묶음 16은 불완전 압축 프레임 수와 일부 출력 나이 표본은 낮았지만, 낮은 FPS 구간·RTT·복구 키프레임은 더 나빴다. **현재는 UDP 자동 설정을 유지한다.** 반복 교차 실험이 아닌 한 차례 순차 비교이므로 일반적인 우열이나 인과관계를 확정하지 않는다. 수신 누락과 복구 반복은 확인했지만 Wi-Fi, VPN 경로, 디코더 정체 각각의 기여도는 분리하지 못했다. UDP/P2P라는 사실만으로 성능 합격을 보장하지 않는다.

판정 원본: [`performance-comparison.json`](../artifacts/lifecycle-repair-2026-09-16/performance-comparison.json), `p1-auto-*`, `p2-custom16-*`.

### 원본 화면 기록의 구분

정지된 다른 브라우저 문서도 같은 측정 서버에 상태를 보내 원본 로그가 섞였다. 실제 공유한 Arc 문서의 navigation time origin으로 문서를 구분했으며, 그 문서의 불리한 상태를 포함해 모두 유지했다. 표시 여부·FPS·실행 여부를 필터 조건으로 사용하지 않았다. 혼합된 원본과 제외 수, 선택 규칙은 `performance-source-binding.json` 및 각 `source-binding-filter.json`에 보존했다.

## 자동 검사와 설치본

- Viewer TypeScript: **39개 파일, 464개 테스트 통과** (`viewer-tests-final.log`).
- Android: **139개 통과**, 실패·오류·건너뜀 0 (`replacement-build-receipt.json`). 합계 **603개**.
- 저장소 타입 검사 통과 (`typecheck-final.log`).
- 저장소 루트 React Doctor **100 / 100**, 발견 사항 없음 (`react-doctor-final.log`). 이후 타입 검사와 관련 테스트를 다시 실행했다.
- Android release 빌드·설치 성공. 기기에 설치된 APK를 다시 읽어 로컬 빌드와 SHA-256 일치를 확인했다 (`installed-replacement.json`).
- 이전 Rust·Swift 검사 결과를 이번 603개에 더하지 않았다. 이번 수정에 Host/Rust 소스 변경은 없다.

| 대상 | SHA-256 |
| --- | --- |
| 최종 Viewer 설치본 | `072d8faa0076d3d057625d70ebed154afc11688b07eca0d4fcbfe639d27e5027` |
| 181초 성능 비교에 사용한 Viewer | `99f12781324b77f57801f84d5237d12be26685112de7872b9370f966065d724d` |
| Host 실행 파일 | `f0121d907ea966cdc24e04c770168a09503938df60702f1305a6536aa6c6a0d7` |
| Host capture shim | `79a094130b527b2d75f1a5b59ae041ad75e17eeaaa904d3c05daaec78fd575e9` |

성능 비교 이후 종료 경로와 이벤트 식별 수정을 추가했다. 따라서 최종 설치본의 고동작 성능 검증 완료로 이전 APK 측정을 대체하지 않는다.

## 아직 합격으로 판정하지 않은 항목

1. **실제 Caps Lock/IME 전환, 두 손가락 스크롤, 가운데 클릭**: 이번 실행에서 사용자 물리 조작을 받지 못했다. ADB 키 입력과 단위 테스트로 물리 키보드 동작을 대체하지 않는다.
2. **외부 인터넷·중계 경로**: 같은 LAN의 Tailscale 시험이며 외부망 시험이 아니다. 다른 인터넷 연결로 전환한 결과가 아직 없다.
3. **장시간 안정성**: 짧은 성능 기준부터 실패했다. 장시간 합격으로 표시하지 않는다.
4. **H.264 대 HEVC의 1440p 실제 비교**: 앞선 준비 실패 뒤 유효한 동등 조건 비교를 확보하지 못했다. 디코더 지원 조회나 4K split 동작은 이 비교를 대체하지 않는다.
5. **Parsec 수준·물리 입력 지연**: 직접 비교와 입력→패널 표시 측정이 없어 미판정이다.

시험 중 불필요한 구버전 Gradle 변환 캐시 약 3.5GB를 정리했다. 앱 데이터·사용자 설정을 초기화하지 않았다. 고동작 시험 탭은 정지 후 닫았다. 최종 설정·활성 공유 정리 결과는 완료 영수증에 남긴다.

최종 설정은 **균형 1440p60, UDP 자동**이다. 시험 공유는 모두 종료했고, 메인 목록에는 새 공유를 열기 전 상태만 남겼다.

## 로컬 커밋 제안

기존 작업과 섞인 파일은 이번 변경 부분을 구분해 스테이징하고, 관련 없는 기존 변경은 보존한다. 아직 커밋·푸시는 실행하지 않았다.

1. `fix(viewer): 메인 창 없이 영상 복구와 공유 종료 유지`
   - `apps/viewer-expo/src/`: `session.ts`, `session.test.ts`, `catalog-helpers.ts`, `catalog-helpers.test.ts`, `reserved-stream.ts`, `reserved-stream.test.ts`, `stream-termination-policy.ts`, `stream-recovery.ts`, `stream-recovery.test.ts`, `stream-session-store.ts`, `use-catalog-model.ts`, `use-stream-controller.ts`
   - `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/`: `StreamActivity.kt`, `StreamLauncherModule.kt`
   - `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/StreamSplitResumeTest.kt`
2. `docs(validation): 창 복구 실기와 1440p 비교 결과 기록`
   - `docs/2026-09-16-window-recovery-followup.md`

원본 측정 자료는 로컬 `artifacts/`에 보존한다. 성능 미합격과 물리 입력·외부망 미검증을 커밋 완료와 혼동하지 않는다.
