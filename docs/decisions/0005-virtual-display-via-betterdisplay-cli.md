# ADR-0005: 가상 디스플레이는 BetterDisplay CLI 옵트인 실험으로 제공

- 상태: 승인 — 옵트인 실험 도입에 대한 승인이며, 정식 기능 승격은 별도 결정으로 유보된다(아래 결정 4번과 정합)
- 날짜: 2026-09-01
- 관련: ADR-0003 (창 스트림 우선), 09-risk-register.md R-015 (가상 디스플레이 scope 팽창 위험)

## 상황

R-015는 가상 디스플레이를 v1 논골로 지정했다. 자체 구현은 DriverKit 기반 신규 프로젝트로 "큰 새 프로젝트"다(출처: 09-risk-register.md §7 R-008/R-012 상세). 사용자 요구로 "창을 드래그해 옮길 수 있는 진짜 확장 모니터" 경험이 필요해졌다.

BetterDisplay는 가상 디스플레이 생성을 CLI로 프로그래밍 방식 지원한다 (BetterDisplay 4.3.6 CLI 헬프 기준):

```text
betterdisplaycli create -devicetype=virtualscreen -virtualscreenname=<이름> -aspectWidth=<W비율> -aspectHeight=<H비율>   # 생성
betterdisplaycli set -namelike=<이름> -connected=on   # 연결
betterdisplaycli discard -namelike=<이름>   # 제거
```

생성·연결된 가상 디스플레이는 macOS에서 진짜 모니터로 열거되므로 Leftcar의 기존 list_displays/캡처/스트리밍 경로가 무수정으로 동작할 것으로 기대된다. 이 기대와 정밀 해상도 지정 방식은 실기기 검증 대상이다.

이 ADR은 ADR-0003의 재검토 조건에 따른 별도 검토다.

## 결정

- 가상 디스플레이는 BetterDisplay(서드파티, 유료 Pro 기능 포함)의 `betterdisplaycli`를 프로세스 실행으로 감싸는 옵트인 실험으로 제공한다.
- BetterDisplay를 번들하지 않는다. 설치 감지 후 미설치 시 안내+링크만 제공한다.
- macOS 전용이다. Windows는 범위 밖이다.
- 기본값은 꺼짐이다. 정식 승격은 실기기 검증 후 별도 결정으로 유보한다.

## 결과

- 긍정: 기존 캡처 경로를 무수정 재사용하므로 구현이 소규모다. DriverKit 프로젝트(R-015의 "큰 새 프로젝트")를 회피한다.
- 부정: 서드파티 앱과 라이선스에 의존한다. CLI 계약이 변경되면 대응이 필요하다. BetterDisplay 설정에서 CLI 접근을 사용자가 수동 허용해야 한다. BetterDisplay 미설치 사용자에게는 기능이 보이지 않는다.
- 중립: ADR-0003(창 스트림 우선)은 유지된다 — 가상 디스플레이는 옵트인 보조 경로다. 이 ADR은 v1 논골(R-015)을 정식 기능으로 뒤집지 않는다.
