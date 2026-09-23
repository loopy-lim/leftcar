# 화면 확장 사용성 구현 계획

> **For agentic workers:** Use executing-plans for inline execution. Check each task against its actual tests before completion.

**Goal:** 확장 화면을 쉽게 열고 조정하며 배치 변경 후에도 입력과 커서가 정확하게 동작하도록 한다.

**Architecture:** 기존 단일 CGVirtualDisplay manager와 스트리밍 경로를 유지한다. 상태/수명/배치 기능을 manager에 모으고 UI는 검증된 상태와 명시적 동작을 사용한다.

**Tech Stack:** Rust/Tauri, Objective-C/Swift, React/React Native, Vitest/Playwright.

**Spec:** `docs/superpowers/specs/2026-09-22-sidecar-extension-experience.md`

## Global Constraints

- 단순 스트림 닫기나 네트워크 단절로 작업 공간을 삭제하지 않는다.
- 공개 API로 다른 모드를 선택하지 않는다. 배치 트랜잭션에서 현재 모드만 유지한다. 이전 화면의 제거 완료 전 재생성하지 않는다.
- LAN/외부 전송 정책, 기존 입력 승인, 사용자 데이터와 페어링을 보존한다.
- React Doctor 100/100 및 typecheck/관련 테스트를 충족한다.
- 커밋은 검증 후 파일·메시지를 제시하고 저장소 지침의 승인을 따른다.

## Task 1 — 수명과 모드 상태

Files: `src-tauri/src/virtual_display.rs`, `VirtualDisplay/CGVirtualDisplayBridge.m`, `Tests/VirtualDisplayBridgeProbe.swift`.

- [x] 기존 phantom probe로 생성/제거 지연을 재현한다.
- [x] 실제 active display 상태로 pending을 판단하고 pending 중 create를 거부하는 회귀 테스트를 추가한다.
- [x] 객체 소유권과 모드 보고를 교정하고 단일 생성/제거/재생성 상태를 검증한다.

## Task 2 — 배치와 입력

Files: virtual display bridge/manager, `CaptureSession+Input.swift`, `CaptureSession+Cursor.swift`, `CursorStreamCoordinator.swift`, Swift tests.

- [x] 스트림 도중 bounds가 이동했을 때 커서 패킷 좌표가 바뀌는 테스트를 RED로 만든다.
- [x] 현재 display bounds를 공유하는 입력 경로 및 커서 갱신을 구현한다.
- [x] 배치 방향을 검증하고 실제 Mac에서 primary/물리 화면 보존을 확인한다.

## Task 3 — 상태 계약과 원격 작업

Files: `control-contract/src/host.rs`, `src-tauri/src/control.rs`, `lib.rs`, Viewer control/model.

- [x] 지원/미지원, 승인 없는 호출, 기존 source 재사용, 늦은 응답 취소, 제거 완료 후 재시도 테스트를 작성한다.
- [x] additive capability/state와 create/open/arrange/size 동작을 연결한다.
- [x] 기존/구버전 카탈로그, 페어링, 스트리밍 수명 회귀를 확인한다.

## Task 4 — Host/Viewer 조작

Files: `ExtendedDisplayCard.tsx`, Viewer extension component/model, `i18n.ts`, `tools/ui-regression`.

- [x] 실제 Host 카드에서 16:9 추천 생성, 선택 시 무동작, 제거 pending 표시 회귀를 RED로 확인한다.
- [x] 자동/프리셋/직접 입력과 명시적 생성·크기 적용, 열기/설정/제거 동작을 구현한다.
- [x] 실제 컴포넌트와 호스트 변경 취소 테스트를 통과시킨다.

## Task 5 — 종합 검증과 전달

- [x] Rust/Swift/TS 관련 테스트, typecheck 및 React Doctor를 실행한다.
- [x] 최종 Mac/Android 후보 패키징 및 동일 소스·해시 확인. Mac geometry 실기와 Android 145개 자동 검사 통과.
- [x] 같은 서명의 새 후보로 Host/태블릿 업데이트, 기존 데이터·Host 페어링 및 입력 권한 유지 확인.
- [x] 태블릿 기본 흐름 재검증: 실제 연결 화면 불일치 해소, 생성·영상·닫기/재열기·크기 변경/복원 및 ADB 클릭·드래그·한영 입력 확인.
- [ ] 전체 태블릿 인수: ADB Command+A 실패 원인 분리, 물리 조합키·다중 터치·펜·잠자기 및 LAN/외부 장시간 검증은 남음. 이번 요청은 태블릿 재검증까지로 한정.
- [x] 변경 전체를 검토하고 발견한 중요 결함을 회귀 테스트로 고친다.
- [x] 현재 증거·제약·후속 실행 순서를 `../specs/2026-09-22-sidecar-extension-verification.md`에 기록한다.
- [x] 최종 커밋 계획에 사용자 승인을 받음. native 수정과 확장 UI/계약·문서의 두 로컬 커밋으로 정리.
