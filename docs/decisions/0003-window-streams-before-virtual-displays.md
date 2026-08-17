# ADR-0003: 가상 디스플레이보다 앱 창 스트림 우선

- 상태: 제안
- 날짜: 2026-08-17

## 상황

사용자는 여러 모니터처럼 여러 앱을 동시에 보고 싶다. 실제 OS 가상 디스플레이는 이 문제의 한 해법이지만 driver, signing, display topology, login/session, HDR, GPU 호환성까지 큰 범위를 만든다.

## 결정

v1은 물리 디스플레이 캡처와 앱 창 캡처만 제공한다. 사용자가 고른 각 앱 창을 Viewer의 독립 stream window로 매핑한다.

## 결과

- macOS ScreenCaptureKit과 Windows.Graphics.Capture의 공개 API로 시작할 수 있다.
- kernel/display driver 없이 사용자 승인 흐름을 유지한다.
- 최소화된 창이나 off-screen 창의 제약이 있을 수 있다.
- “호스트가 실제로 추가 모니터를 인식해야 하는 앱”은 지원하지 않는다.

## 재검토 조건

다음이 실사용에서 확인될 때 별도 ADR로 virtual display를 검토한다.

- 특정 앱이 물리/가상 디스플레이 전체 화면에서만 원하는 레이아웃을 제공한다.
- 최소화/가림으로 창 캡처가 지속되지 않는다.
- 독립 해상도/DPI를 가진 새 desktop surface가 제품 핵심이 된다.

