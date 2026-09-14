# Parsec 격차 프로젝트 기준선 (2026-09-14)

배치 E1 산출물. 측정 프로토콜과 수치 기록 — 이후 배치(A 비트레이트, C 부드러움)는
이 문서의 숫자와 비교해 통과 여부를 판정한다.

## 측정 환경

- 호스트: Mac (Leftcar Host, 고정 서명 빌드 2a872ad), 유선 아님 — Wi-Fi
- 뷰어: Lenovo TB710FU (internal APK), 동일 Wi-Fi, 단일 세션 2560x1440@60
- 수집: `tools/stream-stats.py --device TB710FU --log <file>` 1Hz
- 판정: `tools/stream-gate.py --log <file>` (신뢰 등급 게이트만 사용)

## 게이트 스크립트 판정 규칙 (신뢰도 분석 반영)

| 게이트 | 규칙 | 근거 |
|---|---|---|
| liveness | 세션 ID 1개 유지 + state==running | 세션 ID 변경 = 재접속 발생 |
| render | cap>0 표본의 90%+에서 rend ≥ 0.5·fpsTarget | 정적 화면은 cap=0 정상 — 움직임 표본에서만 판정 |
| loss | receiverFrameGaps 델타 = 0 | 누적 카운터 diff, NACK 치유분 미포함 하한 |
| recovery-budget | recoveryKeyframes 델타 ≤ 0 (기본) | 복구 발생 자체가 히치 원인 |
| bitrate-floor | bitrateFloorCollapseCount 델타 = 0 | 바닥 붕괴는 긴 복구를 뜻함 |
| send-health | udpSendFailures 델타 = 0 | 소켓 전송 실패 |

## 기준선 1 — 안정 구간 (22:23, 45s, 화면 거의 정적)

세션 시작 직후. 6/6 게이트 통과. rend 42–56, gaps 0, recov delta 0, kbps 0–1(정적
화면이므로 정상).

## 기준선 2 — 인터랙션 구간 (22:39–22:40, 60s)

사용자 창 조작 + 병렬 에이전트 창 갱신으로 실손실 에피소드가 섞인 구간. 4/6 통과.

- liveness PASS: 세션 18분+ 연속 생존 (시작 레이스 수정 전에는 6초 사망)
- render PASS(91%): rend min=2 / max=56 — **복구 히치 시 2fps까지 떨어짐(배치 C 표적)**
- loss FAIL: gaps 델타 5 (FEC+NACK으로 못 막은 손실)
- recovery-budget FAIL: recoveryKeyframes 델타 14 — 손실 에피소드마다 복구 진입
- 배치 C 목표치(제안): 같은 조건에서 recovery 델타 감소 + rend 최저치 상향(2 → ≥24)

## 배치 A 이전 화질 기준선

- 시작 비트레이트: idealBits = w·h·fps·0.07 → 1440p60 ≈ 15.5Mbps (로그 확인:
  bitrate=15482880), 호스트 ABR 상승은 clean 8윈도우 이후
- 비4K 상한 28Mbps, quality hint 0.5가 0.55–1.0x 추가 스케일
- 움직임 구간 Mbps 실측은 배치 A 착수 시 정적/움직임 구분 수집으로 보강한다

## 배치 A 결과 (2026-09-14, 커밋: 비트레이트 정책)

변경: 시작 계수 0.07→0.13, 비4K 비디오 상한 28→45Mbps(다중 20→30), 상승 래더
8→3 클린 윈도우, 혼잡 후 천장 완화 30→8 윈도우. 측정(window 손실 버스트 포함):

- 시작 currentBitrate **31.0Mbps**(이전 15.5) 즉시 적용 확인
- 손실 버스트 구간에서 ABR이 설계대로 7.7M까지 컷(빠른 절단 승인 정책) —
  그리고 새 래더로 30초 내 27→31M 회복, 이후 천장 45M 향해 가속 상승
- 게이트: liveness·render(95%)·bitrate-floor·send-health PASS,
  loss/recovery FAIL은 실제 Wi-Fi 손실 구간(스스로 복구, frameGaps 28에서 동결,
  FEC 치유 1468·미복구 0) — 배치 C의 개선 표적으로 기록
- 세션은 배치 0 수정 후 한 번도 끊어지지 않음(40분+ 연속)

## 절차 유의사항

- getStatus의 뷰어 카운터는 호스트가 max() 병합 — 뷰어 재시작 뒤 손실은 옛
  최댓값을 넘을 때까지 안 보인다(스크립트는 델타만 판정)
- wire/age/inRtt, idrRes, FEC 통계는 단일 세션에서 전송되지 않는다(배치 E2 확장 대상)
- 첫 stream-stats 실행 시 키체인 동의 1회 필요(항상 허용 후 비대화형)
