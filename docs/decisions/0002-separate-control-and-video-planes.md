# ADR-0002: 제어 경로와 영상 경로 분리

- 상태: 제안
- 날짜: 2026-08-17

## 상황

Rustra는 Rust command에서 TypeScript client를 생성하고 여러 host adapter를 제공한다. Leftcar는 이 장점을 쓰고 싶지만, 압축 영상은 초당 수십에서 수백 MB의 burst와 엄격한 backpressure 정책을 가진다.

## 결정

- Rustra는 로컬 UI와 Rust core 사이 command, event, typed error에 사용한다.
- Host와 Viewer 사이 network control protocol은 별도 버전 계약을 가진다.
- 압축 영상 access unit은 native video plane으로 전송한다.
- 영상 byte는 JSON, TS object, Rustra command/event를 거치지 않는다.

## 결과

- UI GC와 영상 backpressure가 분리된다.
- Rustra schema 변경과 network media protocol 변경을 독립적으로 배포할 수 있다.
- 계측과 테스트 경계가 분명해진다.
- 대신 control/video session correlation과 오류 합성이 필요하다.

## 검증

- 코드 구조 테스트가 video crate에서 `rustra`와 JS runtime dependency를 금지한다.
- 성능 trace가 encode output에서 MediaCodec input까지 TS/JS frame이 없음을 보여야 한다.

