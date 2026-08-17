# ADR-0001: Home Space 멀티 인스턴스 뷰어

- 상태: 제안
- 날짜: 2026-08-17
- 결정자: 제품 책임자 승인 필요

## 상황

원격 앱 여러 개를 Galaxy XR에서 각각 독립 창으로 보고 싶지만, OpenXR나 공간 렌더링 기능은 사용하고 싶지 않다.

## 결정

일반 Android 앱의 multi-instance/task 기능을 사용한다.

- HubActivity는 연결과 source 선택을 담당한다.
- source 하나를 열 때마다 새로운 `StreamActivity` document task를 만든다.
- Android XR Home Space가 각 task를 별도 2D 창으로 배치한다.
- 창마다 decoder와 Surface가 독립적이다.
- transport, 인증, source catalog는 앱 프로세스의 공용 세션이 공유한다.
- 한 창 Overview는 fallback이자 선택 기능이다.

## 이유

- Android XR 공식 사용자 안내가 같은 앱의 여러 창을 Home Space에서 지원한다.
- Android 15 multi-instance System UI opt-in이 존재한다.
- 다른 Android 앱과 동시에 사용할 수 있다.
- XR SDK와 Full Space의 API/배포 위험이 없다.
- source와 window lifecycle을 자연스럽게 일대일 대응할 수 있다.

## 결과

좋은 결과:

- 사용자가 각 원격 앱 창을 직접 이동, 크기 변경, 닫기 할 수 있다.
- 각 stream 장애를 다른 창과 격리하기 쉽다.
- 창 하나에 여러 decoder를 합성하는 UI가 필요 없다.

비용:

- Activity/task lifecycle과 process restoration을 정확히 처리해야 한다.
- 같은 프로세스의 여러 Surface/decoder가 자원을 경쟁한다.
- 시스템 창 배치를 앱이 정밀 제어할 수 없다.
- Galaxy XR의 실제 동시 창/decoder 한계를 측정해야 한다.

## 기각한 대안

- 단일 Activity 2x2 타일: 구현은 쉽지만 독립 창 요구를 충족하지 않는다.
- Full Space SpatialPanel: 독립 공간 패널은 가능하지만 XR SDK가 필요하고 다른 앱이 숨겨진다.
- 원격 앱마다 별도 APK: 배포, 업데이트, 권한, 서명 관리가 지나치게 복잡하다.

## 검증

F-01부터 F-03 실험을 Galaxy XR 실기기에서 통과해야 상태를 `승인`으로 바꾼다.

