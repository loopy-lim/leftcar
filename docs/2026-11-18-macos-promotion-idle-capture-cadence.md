# macOS(ProMotion) 정적 화면 캡처 cadence 저하 — 60fps 미달 원인 규명 (2026-11-18)

요청: 맥북(M1 Max) 호스트에서 뷰어 HUD fps가 60에서 가끔 40/30으로 떨어지는
현상의 원인 확인. 리눅스 NVENC 경로(docs/plans/2026-11-02-linux-nvenc-60fps.md
V-7)와는 별개 항목이다.

## 결론

**ProMotion 가변 주사율 + ScreenCaptureKit damage 기반 deliver의 물리 한계다.
인코더·페이서·네트워크·뷰어 디코더는 모두 정상이다.**

- 화면 내용이 정지하면 macOS 컴포지터가 디스플레이 업데이트율을 내리고
  (ProMotion LTPO 램프다운), SCK는 컴포지터가 프레임을 만들어야 deliver하므로
  **캡처 가능한 프레임 수 자체가 줄어든다**.
- 호스트는 캡처된 만큼만 인코딩·송출하고, 뷰어 HUD(Surface 렌더 카운터 기반
  윈도우 레이트)가 그 숫자를 정확히 반영한다. 측정 오류가 아니다.
- 모션 중에는 55~61fps(사실상 60 달성, 프레임 간격 16.7ms 정상 cadence)다.

## 측정 증거

프로브: 호스트와 동일한 SCStreamConfiguration(minimumFrameInterval=1/62,
420f, showsCursor=false, queueDepth 3/8, 1920x1080 다운스케일)로 프레임
도착 간격을 직접 카운트. 인코더·네트워크·뷰어 무관.

- `artifacts/fps60-mac-2026-11-18/probe/sck_q3.jsonl` (queueDepth 3, 60s):
  - idle 41~52fps, 인터벌 20~28ms(≈36-50Hz) 다수. 시간에 따라 변동.
  - motion(60fps 애니메이션 창) 55~60fps.
  - idle 복귀 직후 58~61 유지 → 서서히 하락하는 ProMotion 램프다운
    히스테리시스 관찰.
- `artifacts/fps60-mac-2026-11-18/probe/sck_q8.jsonl` (queueDepth 8, 45s):
  idle 50~59fps — 같은 저 cadence 상태(20~24ms 모드)가 나타나지만 q3보다
  얕다. 단 회차가 달라 컴포지터 상태가 섞여, q3 vs q8 우열은 단정하지
  않는다(지배 변수는 어디까지나 컴포지터 cadence).
- 기기: Apple M1 Max, Built-in Liquid Retina XDR(ProMotion). 저전력
  모드 꺼짐(pmset powermode 0). 호스트 캡처 백엔드는 SCStream이 기본
  (CGDisplayStream은 구버전 폴백, CaptureSession+Backend.swift:128/207).

## 2차 영향 검토

- 뷰어 적응형 해상도 컨트롤러는 "fps 저하 단독으로는 강등하지 않음"으로
  이미 설계돼 있어(adaptive-resolution.ts isCongested 주석, 손실·복구·
  큐 압력이 동반될 때만 강등) 정적 화면 cadence 저하가 해상도 강등을
  유발하지는 않는다.
- 커서는 LCD1 평면(60Hz 폴링)으로 별도 전송되므로 정적 화면에서의
  포인터 응답성은 영향 없음.

## 결정 (사용자, 2026-11-18)

dup-frame 재인코딩으로 60 cadence를 유지하는 대신 **낮은 fps를 그대로
두고 HUD가 "유휴" 상태로 표시**하기로 했다. 근거: 정적 화면에서 잃을
프레임이 없고(끊김 체감 없음), 상시 재인코딩의 배터리·비트레이트 비용과
"실제 원본 cadence를 왜곡해 60으로 표시"하는 부작용이 이득보다 크다.

구현: 뷰어 HUD fps 배지에 유휴 판별 상태 표시(저 fps + 손실·갭 증가 없음
→ "유휴"). 별도 커밋으로 반영.
