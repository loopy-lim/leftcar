# Leftcar

Leftcar는 macOS와 Windows의 화면 또는 애플리케이션 창을 Galaxy XR에서 빠르게 보는 것을 목표로 하는 읽기 전용 다중 화면 뷰어 프로젝트다.

이 저장소는 현재 **설계와 검증 계획만 포함**한다. 제품 코드는 아직 구현하지 않는다. 구현자는 [문서 인덱스](docs/README.md)부터 읽고, 검증 게이트를 통과한 뒤 기술 선택을 확정해야 한다.

## 한 문장 정의

> 사용자가 선택한 데스크톱 화면이나 앱 창 여러 개를 로컬 네트워크로 전송하고, Galaxy XR Home Space에서 각각 독립된 Android 창으로 낮은 지연으로 보여 주는 도구

## 고정된 기본 범위

- 화면 보기 전용이다. 키보드, 마우스, 터치 원격 제어를 구현하지 않는다.
- Android XR 전용 기능 없이 Home Space에서 실행되는 일반 Android 앱으로 시작한다.
- 원격 source 하나를 Android Activity/task 인스턴스 하나에 연결해 여러 독립 창으로 보여 준다.
- 선택적인 Hub 창은 연결과 source 선택을 담당하고, 선택적인 Overview 창만 여러 타일을 한 화면에 모은다.
- 실제 OS 가상 모니터 드라이버보다 앱 창 또는 물리 디스플레이 캡처를 먼저 지원한다.
- Rustra는 Rust와 TypeScript 사이의 명령, 상태, 오류 계약에 사용한다.
- 압축 영상 데이터는 Rustra나 JavaScript를 통과시키지 않고 별도 네이티브 데이터 경로로 전송한다.
- 제품 로직은 TypeScript와 Rust로 작성한다. Kotlin은 Activity, Intent, Surface를 연결하는 고정 Android shim으로만 제한한다.
- 로컬 네트워크 직접 연결을 첫 배포 범위로 한다.

## 문서

- [문서 인덱스](docs/README.md)
- [제품 요구사항](docs/01-product-requirements.md)
- [기술 타당성 조사](docs/02-feasibility-research.md)
- [시스템 아키텍처](docs/03-system-architecture.md)
- [Rustra 제어 계약](docs/04-rustra-control-contracts.md)
- [TDD와 품질 전략](docs/05-tdd-quality-strategy.md)
- [성능 측정 계획](docs/06-benchmark-device-validation.md)
- [보안과 개인정보 보호](docs/07-security-privacy.md)
- [24주 구현 계획](docs/08-implementation-roadmap.md)
- [위험과 미결정 사항](docs/09-risk-register.md)
- [공식 근거 자료](docs/10-references.md)

## 현재 상태

- 조사 기준일: 2026-08-17
- 상태: 계획 승인 전, 구현 시작 전
- 우선 대상 호스트: macOS
- 두 번째 대상 호스트: Windows
- 선택적 후속 대상: Linux
- 우선 대상 뷰어: Samsung Galaxy XR 실기기
