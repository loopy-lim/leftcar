# 남은 검증 실행 결과 — 2026-09-16

## 결론

**실행한 자동 검사는 통과했지만, 실기 성능과 창 독립성은 전체 합격이 아니다.** 새 Host 전송 시간 예산을 설치한 상태에서 Lenovo TB710FU로 확인했다. 이번 작업에서는 제품 코드나 설치본을 변경하지 않았다.

| 항목 | 이번 판정 | 확인한 범위 |
| --- | --- | --- |
| 자동 테스트 | PASS | TypeScript 59 + Rust 264 + Android 136 + 측정 도구 12 = 471개 |
| 타입 검사 / React Doctor | PASS | 타입 검사 성공, React Doctor 100 / 100 |
| 1440p60 고동작 181초, UDP 자동 | FAIL | RTT·프레임 손실·복구 기준 미달, 출력 로그 연속성도 불충분 |
| 사용자 지정 묶음 16 비교 | 비교 제외 | 측정 중 원본 화면이 숨겨지거나 포커스를 잃은 표본 5개 |
| 선명한 화면 시작 | 기능 PASS | 3840×2160 split 영상 출력 확인; 성능 합격 아님 |
| 선명한 화면에서 홈으로 이동 후 복귀 | 기능 PASS | 같은 영상 창에서 새 Host 세션으로 복구 |
| 메인 창 제거 후 선명한 화면 복귀 | FAIL | 영상 창은 남지만 검은 화면으로 멈춤 |
| H.264 / HEVC 디코더 기능 조회 | PASS | 기기가 하드웨어 저지연 디코더와 1440p60 지원을 보고함 |
| H.264 실제 스트리밍 비교 | 미검증 | 직접 재설정 요청이 UDP 도달 확인에서 실패해 유효한 시험이 시작되지 않음 |
| 실제 Caps Lock 한·영, 두 손가락, 가운데 클릭 | 미검증 | 이번 실행에서 실제 물리 입력을 받지 못함 |
| 외부망 / 장시간 / Parsec 직접 비교 | 미검증 | 이번 경로는 같은 LAN의 Tailscale 직접 연결. 짧은 성능 기준부터 실패 |

원본 로그와 JSON: [`artifacts/final-acceptance-2026-09-16/`](../artifacts/final-acceptance-2026-09-16/).

## 1. 1440p 자동 전송 성능

`d1-auto-metrics.json`, `d1-auto-gate.json`, `d1-auto-boundaries.json`이 판정 원본이다.

- 2560×1440, 목표 60fps, 실제 HEVC 저지연 디코더. 상태의 `rateControl` 값은 압축 형식 판별에 사용하지 않았다. 네이티브 로그에서 `c2.qti.hevc.decoder.low_latency`를 확인했다.
- UDP 자동: 묶음 4, FEC 2, adaptive pacing 켜짐. 상세 프레임 trace 꺼짐.
- 유효 측정 181.114초. 원본 고동작 화면은 전체 구간 표시·포커스·전체 화면을 유지했고 약 59.97fps로 움직였다. 태블릿 전경 표본도 모두 StreamActivity였다.
- RTT 상태 표본 p50 **25ms**, p95 **47ms**: 기준 p95 20ms 이하 실패.
- 새 프레임 gap **8**, 불완전 AU **38**, 복구 키프레임 **46**: gap·복구 0 기준 실패.
- 송신 실패와 비트레이트 하한 압박은 각각 0.
- 렌더 상태 표본 181개 중 **107개**만 목표의 95%인 57fps를 만족했다. 초반 부진을 잘라내지 않고 전 구간을 판정했다.
- capture→소프트웨어 Surface release 나이 표본 307개의 p50은 52ms, p95는 131ms였다. 다만 출력 로그 연속성 조건이 실패했으므로 **전체 구간 평균 출력 fps와 정식 지연 판정은 UNKNOWN**으로 남겼다. 이 표본 p95를 실제 패널 지연 또는 물리 입력 지연으로 해석하지 않는다.

[앞선 Host 내부 UDP 시험](2026-09-16-udp-frame-budget-improvement.md)의 처리량 개선은 유지된 별도 증거다. 이번 무선 시험은 실기 전체 합격을 뒷받침하지 않는다. 수정 전/후 실기를 같은 조건에서 번갈아 측정하지 않았으므로 새 수정의 회귀나 개선 폭도 확정하지 않는다.

## 2. 대안 비교의 한계

### 묶음 16

`d2-custom16-*`은 약 181초를 수집했지만, 원본 cadence 182개 중 5개가 숨김 또는 포커스 이탈이었다. 측정 종료 후에는 태블릿에 다른 프로젝트의 앱도 전경으로 나타났다. 종료 후의 앱 전환과 측정 중의 원본 이탈은 별개 관찰이다. `d2-exclusion.json`에 비교 제외 사유를 남겼다.

이 구간에서 보인 출력·지연 숫자로 묶음 16이 더 낫다고 선택하지 않는다. 현재 후보 간 유효한 우열 결론은 없다.

### H.264

기능 조회는 AVC와 HEVC의 하드웨어 저지연 지원 및 1440p60 지원을 확인했다. 기능 조회는 실제 디코더 생성·처리량 검증이 아니다.

직접 `reconfigureStream`으로 H.264 후보를 요청했지만 `UDP reachability proof failed`로 대체 스트림 준비가 실패했다. 기존 스트림도 종료되었다. 새 Viewer 수신기 준비를 포함한 정상 연결 절차로 다시 시험해야 한다. **H.264 디코더 성능 실패나 미지원 판정으로 해석하지 않는다.** 원본: `codec-capabilities.json`, `h264-reconfigure.json`, `h264-smoke.jsonl`.

## 3. 선명한 화면과 창 분리

일반 데스크톱 장면에서 3840×2160 split 영상을 시작했다. 시작 10개 상태 표본의 출력은 52~60fps였다. 고동작 장면이 아니므로 4K60 성능 합격에는 사용하지 않는다.

### 홈 이동 후 복귀: 복구 확인

1. 메인 창과 영상 창이 있는 상태에서 Android 홈으로 이동.
2. 32초 상태 기록 동안 기존 Host 세션 6은 feedback 중단 후 정리됨.
3. 최근 앱에서 같은 영상 task 598로 복귀.
4. 새 Host 세션 7이 준비되고 4K 영상 출력이 재개됨.

복귀 후 16개 표본 모두 실행 중이었다. **끊김 없는 백그라운드 전송을 증명한 것이 아니라 새 연결로 복구된 결과**다. 근거: `clarity-start.jsonl`, `clarity-background.jsonl`, `clarity-return.jsonl`, `clarity-return.png`.

### 메인 창 제거 후 복귀: 실패 재현

1. 최근 앱에서 MainActivity 카드만 닫음.
2. `main-removed-activities.txt`에서 Leftcar MainActivity 제거와 영상 task 598 잔존 확인.
3. 남은 영상 창으로 복귀.
4. 기존 세션 7은 error 뒤 제거되었고 영상 창은 검은 화면으로 남음. 추가 관찰 후에도 출력 없음.

`main-closed-lifecycle.log`에는 `split render recovery requires React/Host re-preparation`이 기록되어 있다. **메인 화면의 React 연결 수명에 재연결이 의존하는 것이 유력한 원인**이지만 세부 실패 지점은 추가 추적이 필요하다. 메인 창과 화면 목록을 다시 열자 새 세션 8로 영상이 돌아왔다.

정리 과정에서는 화면 닫기 후 목록의 기존 공유 항목이 남고 새 영상이 다시 열리는 현상도 관찰했다. 메인 창 부재 중 닫기 이벤트와 자동 복구가 충돌하는지 함께 확인해야 한다. 시험 스트림을 Host에서 명시적으로 정지한 뒤 Viewer 프로세스를 다시 실행해 정리했다. 앱 데이터는 지우지 않았다.

## 4. 자동 검증과 재현성

- `bun run typecheck`: 성공 (`typecheck.log`).
- 관련 TypeScript 테스트: 5개 파일 59개 성공 (`typescript-tests.log`).
- `cargo test -p android-viewer -p keymap --locked`: 260 + 4개 성공 (`rust-tests.log`).
- `python3 -m unittest tools/test_stream_gate.py`: 12개 성공 (`measurement-tests.log`).
- Android `:app:testReleaseUnitTest -PleftcarInternalBuild=true`: 136개 성공, 실패·오류·건너뜀 0 (`android-tests-internal.log`, `android-counts.json`). 첫 실행은 내부 빌드 플래그 누락으로 설정 단계에서 실패했고 올바른 플래그로 재실행했다.
- 저장소 루트 React Doctor: 100 / 100, 발견 사항 없음 (`react-doctor.log`). 이번 실행에서 제품 소스를 고치지 않았다.
- 앞선 Swift 전송 예산 72개 조합·policy 검사·shim 빌드는 이번에 반복하지 않았다. 관련 파일 8개가 앞선 검증 해시와 일치함만 확인했다 (`prior-source-verification-match.json`). 이를 새로 실행한 471개에 더하지 않았다.

실기 시험 설치본 SHA-256:

| 대상 | SHA-256 |
| --- | --- |
| Viewer APK | `8432ffeb1056c6df042a47fe50c19e57295ec0b6e666c191259c7ac746c45b5d` |
| Host 실행 파일 | `f0121d907ea966cdc24e04c770168a09503938df60702f1305a6536aa6c6a0d7` |
| Host capture shim | `79a094130b527b2d75f1a5b59ae041ad75e17eeaaa904d3c05daaec78fd575e9` |

## 5. 남은 항목과 다음 순서

1. **영상 창만 남은 상태의 재연결 및 닫기 처리 수정**: 메인 UI가 없어도 재연결 준비와 명시적 종료 처리가 살아 있어야 한다. 위 4K 재현을 회귀 시나리오로 사용한다.
2. **1440p 무선 성능 재검증**: 다른 앱의 부하·전경 전환이 없는 구간에서 출력 로그 연속성을 확보하고 UDP 자동/묶음 후보를 교차 비교한다. 단기 기준을 통과한 후보에 장시간 시험을 적용한다.
3. **물리 입력 검증**: Caps Lock/IME 언어 전환, 두 손가락 스크롤, 가운데 클릭은 실제 기기 조작 기록이 필요하다. 이번에는 사용자 조작 응답이 없었고 Mac 테스트 입력칸을 안전하게 포커스하는 UI 도구도 반복 시간 초과로 실패했다. 준비한 주입 도구는 실행하지 않았다. 단위 테스트나 ADB 키 주입을 물리 입력 합격으로 대체하지 않는다.
4. **외부망**: Tailscale 주소로 실제 영상이 연결됐지만 ping 경로는 같은 LAN의 직접 연결이었다. 외부망/중계 경로·재접속 성능은 별도 검증이 필요하다.
5. **Parsec 동등성**: 같은 장면·망·기기에서 직접 비교 및 물리 입력→화면 측정이 없어 아직 판정하지 않는다.

최종 설정과 시험 자원 정리는 같은 artifact 폴더의 `completion-receipt.json`에 기록한다. 전체 상태는 **미합격이며 일부 검증이 남아 있음**이다.
