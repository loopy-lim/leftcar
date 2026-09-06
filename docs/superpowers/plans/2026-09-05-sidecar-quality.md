# Sidecar Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement and review each task.

**Goal:** 지연·글자·영상 품질과 HiDPI 가상 디스플레이 및 Android 창 비율을 완성한다.
**Architecture:** 기존 캡처/인코더/UDP-FEC 경로를 유지하고, 적응 정책·디스플레이 관리·플랫폼 창 API를 확장한다.
**Tech Stack:** Rust, Swift, Kotlin, React/TypeScript, Tauri, Android.
**Spec:** docs/plans/2026-09-05-sidecar-quality-proposal.md (사용자 구현 승인 2026-09-05)

## Global Constraints

- 반응 속도 우선, 글자 가독성 하한 유지, 영상 연속성 검증.
- 실제 기기 결과 없이 실측 통과를 주장하지 않는다.
- React 변경 후 루트 `npx -y react-doctor@latest . --verbose` 100/100 및 타입/관련 테스트 재실행.
- 기존 미추적 사용자 문서를 보존한다. 에이전트는 커밋/푸시하지 않는다.
- 사용자 병렬 에이전트 요청에 따라 수정 파일 소유권을 분리한다. 충돌하는 변경은 통합 담당자에게 넘긴다.
- 사용량 90% 사용(10% 잔여) 한도. 리셋 크레딧 사용은 승인되지 않았다.

### Task 1: Adaptive streaming and clarity

Files: apps/viewer-expo/src/adaptive-resolution.ts 및 테스트, stream-profile.ts 및 테스트, use-stream-controller.ts.
Interfaces: 기존 AdaptiveTarget/Observation 유지 또는 하위 호환 optional 신호 추가. renderer 요청과 실제 수락된 target을 일치시킨다.
- [x] 3200×2000·세로 화면 폴백, 손실 없는 큐 증가, 정지 화면, 실패 재시도 cooldown에 대한 실패 회귀 테스트 작성·실행.
- [x] 비율 보존·짝수 픽셀·하한을 가진 폴백과 idle-safe 혼잡 정책, 수락된 해상도 상태 반영 구현.
- [x] 실제 렌더 정보 없는 단순 확대를 clarity 기본 동작에서 제거하고 UI 설명을 맞춘다.
- [x] 관련 Vitest·typecheck 실행 및 보고.

### Task 2: HiDPI and managed virtual displays

Files: apps/host-desktop/src-tauri/src/{virtual_display,provider,lib,clamshell_mode}.rs, 새로운 display management module, tools/cgvd-shim, apps/host-desktop/src/{App.tsx,새 display UI module}.
Interfaces: 논리 width/height + scale(1|2), 고유 관리 ID, 검증된 backing pixel 크기, position(left|right|above|below). 기존 legacy 명령의 호출 호환성 유지.
- [ ] mode 크기 검증·소유 화면만 제거·다중 화면 독립 수명·배치 계산 회귀 테스트 먼저 작성.
- [ ] 설치된 BetterDisplay CLI 계약 확인 후 HiDPI 모드 생성/선택/검증 구현. CGVD도 같은 논리/픽셀 의미와 고유 식별자로 수정.
- [ ] 관리 목록·추가·개별 제거·위치 변경, 논리 해상도와 HiDPI UI 제공. 기존 단일 세션 정리와 충돌 방지.
- [ ] Host Rust 검사·Swift shim 빌드·UI 타입 검사 실행.

### Task 3: Android proportional windows

Files: apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/*, 필요한 manifest/build 파일과 해당 테스트.
Interfaces: openStream(width,height) 기존 인자 유지. Surface와 입력의 aspect-fit 일치.
- [x] 지원되는 일반 Android/freeform/XR API를 공식 문서에서 확인.
- [x] source 비율과 사용 가능 공간에서 초기 bounds를 산출하는 순수 함수·회귀 테스트 작성.
- [x] 최초 창 크기 요청, 회복 중 사용자 크기 유지, config-change 안전성 구현.
- [x] Kotlin 단위 테스트·Android assemble 및 한계 보고.

### Task 4: Integration and physical validation

Files: 변경 필요 시 작업 소유권 인계 후 처리; docs/ 검증 결과.
- [ ] 각 작업 독립 리뷰 및 지적 수정.
- [ ] 실제 ADB/무선·Mac 활성 디스플레이·Host 앱 상태 확인. 필요한 사용자 기기 조치는 즉시 요청하되 독립 구현 계속.
- [ ] React Doctor, 타입, TS·Rust·Swift·Kotlin 관련 검사 및 Host/APK 빌드.
- [ ] 연결 가능 시 동일 움직임/글자 샘플의 기준선과 개선 결과 비교, 180초·10분 및 60분 장시간 검증. 실기 불가 시 정확한 미검증 항목 유지.
- [ ] 요구사항별 증거 감사 후 커밋안 제시; 승인된 경우에만 커밋.

### Task 5: Hot-path correctness found in final audit

- [x] 세로/가로 동일 픽셀의 인코더 bitrate·in-flight·화질 등급 동등성.
- [x] 추가 패킷 없이 재정렬 대기 만료를 처리하고 마지막 화면 변경을 전달.
- [x] 시작/복구 경계에서 먼저 완성된 delta가 뒤늦은 IDR을 버리지 않도록 처리.
- [x] 회귀 테스트, 최종 native rebuild 및 패키지 재검증.

## Current integration status

2026-09-05: Task1/3 구현과 관련 테스트 통과. Android 배포 빌드 R8 문제는 공식 XR compileOnly 플랫폼 API 의존성으로 해결했고, JavaScript 포함 release APK 생성 확인. Task5 native 변경 후 최종 APK를 다시 생성한다.

Task2는 HiDPI 생성과 개별 프로세스 수명 확인, 설치본 리소스/서명/CLI 탐색 검증을 완료했으나, 관리자 프로세스의 신규 CGVD 관측/배치 오류1001을 실기로 진단 중이다. 오류 정리·영속 소유권·UI 복구 표시도 마지막 검토 중이다.

Android 장치는 USB/mDNS 모두 검색되지 않는다. 실제 영상·글자·Galaxy XR 창 조작 및 장시간 테스트는 완료 처리하지 않는다.


2026-09-06 최종 경계: CGVD owner PLACE와 UUID/generation 기반 실제 픽셀 전달은 구현 및 실기 확인했다. 다만 최신 실기에서 제거 후 물리 미러링 화면의 primary UUID가 바뀌었고, BetterDisplay 추가 실기는 자동 승인 검토에서 거부됐다. Android 미연결도 계속된다. 전체 목표는 완료 처리하지 않으며 추가 실기는 승인/연결 정보가 필요하다. Task5와 Android standalone APK는 최종 재빌드·검증했다.
