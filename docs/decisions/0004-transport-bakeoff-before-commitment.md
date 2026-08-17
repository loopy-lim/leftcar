# ADR-0004: 전송 방식은 실기기 bake-off 후 확정

- 상태: 제안
- 날짜: 2026-08-17

## 상황

WebRTC는 성숙한 실시간 미디어 기능을 제공하지만 제어하기 어려운 buffer가 있을 수 있다. QUIC DATAGRAM은 Rust 구현과 최신 프레임 우선 정책에 적합하지만 미디어 전송 기능을 직접 만들어야 한다.

## 결정

문서만으로 하나를 확정하지 않는다. 동일한 H.264 access unit, 동일한 Host/Viewer, 동일한 network profile로 WebRTC와 QUIC prototype을 비교한다.

## 판정 순서

1. 보안 기본 요건
2. correctness와 복구
3. glass-to-glass p95와 latency creep
4. 1/2/4 stream 안정성
5. CPU, battery, thermal
6. 구현, 배포, 관측 복잡도

앞 항목을 만족하지 못한 후보는 뒤 항목이 좋아도 탈락한다.

## 결과

- 초기에 중복 prototype 비용이 든다.
- 근거 없는 저지연 주장을 피한다.
- transport abstraction은 실험을 위해 만들되 제품에서 영구적인 두 구현을 지원할 의무는 없다.

