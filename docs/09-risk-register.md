# 위험 등록부와 미결정 사항

문서 상태: 활성  
갱신 규칙: 매 Gate review에서 probability, impact, evidence, owner, next check를 갱신한다.

## 1. 등급

- 확률: Low, Medium, High
- 영향: Low, Medium, High, Critical
- 상태: Open, Mitigating, Accepted, Closed

## 2. 핵심 위험

| ID | 위험 | 확률 | 영향 | 조기 검증 | 완화/대안 | 상태 |
| --- | --- | --- | --- | --- | --- | --- |
| R-001 | Galaxy XR에서 같은 앱 4창이 예상대로 열리지 않음 | Medium | Critical | H05 | task/manifest 수정, Overview, 승인 후 SpatialPanel | Open |
| R-002 | 비초점 Home Space 창의 Surface 갱신이 throttled/paused | Medium | Critical | H06/H08 | lifecycle 수정, lower FPS, Overview | Open |
| R-003 | hardware decoder 4개 동시 자원 부족 | Medium | High | H08 | focus 1 + 720p15 보조, 창 상한 축소 | Open |
| R-004 | React Native가 여러 Activity/root를 안정적으로 공유하지 못함 | Medium | High | H04-H09 | ReactHost 구조 수정, stream view native shell 최소화 | Open |
| R-005 | Rust NDK AMediaCodec API가 필요한 vendor/low-latency 제어를 충분히 노출하지 않음 | Medium | High | H07 | JNI로 제한된 Java capability/config 호출, 정책은 Rust 유지 | Open |
| R-006 | 최소 Kotlin shim이 점차 business layer로 커짐 | Medium | Medium | H01/PR | allowlist, TS spec, C ABI snapshot, owner review | Mitigating |
| R-007 | WebRTC와 QUIC 모두 SLO 또는 패키징을 만족하지 못함 | Medium | Critical | H11-H14 | profile 축소, 병목 개선 1회, 제품 재승인 | Open |
| R-008 | macOS 최소화/가림/Space 상태에서 창 캡처가 기대와 다름 | High | High | H16-H20 | 사용 조건 문서, display capture, virtual display 재검토 | Open |
| R-009 | 다중 ScreenCaptureKit/VideoToolbox session이 Host 자원 초과 | Medium | High | H17-H18 | adaptive profile, shared budget, source 상한 | Open |
| R-010 | H.264 4:2:0 text 품질이 모니터 용도에 부족 | Medium | High | H20/H21 | bitrate/scale, HEVC, 중요 창 고품질 | Open |
| R-011 | 4창에서 Galaxy XR thermal/battery가 급격히 악화 | High | High | H08/H36 | dynamic FPS/resolution, charger mode, 상한 | Open |
| R-012 | source 창 재생성 시 ID가 바뀌어 잘못 복구 | Medium | High | H16/H29 | 명시적 reselect, revision, title heuristic 금지 | Open |
| R-013 | pairing 또는 source metadata가 log에 노출 | Medium | Critical | H22-H27/H37 | allowlist log, redaction property tests | Open |
| R-014 | Android XR/SDK/System UI 동작이 업데이트로 변경 | High | High | 각 release | 일반 Android API 우선, device matrix, pin/compat test | Accepted |
| R-015 | 실제 virtual display/입력/오디오 요구로 scope가 팽창 | High | High | G0/weekly | 별도 ADR/제품, v1 non-goals 유지 | Mitigating |
| R-016 | Rustra main/adapter API가 빠르게 바뀜 | High | Medium | H02 | vetted commit pin, contract hash, clean generation | Open |
| R-017 | Galaxy XR device lab 접근이 불안정 | Medium | Critical | P0 | 예약, physical gate 전 일정 여유, emulator와 분리 | Open |
| R-018 | Windows GPU/driver matrix가 너무 넓음 | High | High | H40-H43 | supported matrix, no silent software, 우선 GPU 좁힘 | Open |
| R-019 | protected/secure content 기대 오해 | Medium | High | product docs | 명시적 비지원, bypass 금지, source error | Mitigating |
| R-020 | software timestamp가 실제 지연보다 낙관적 | High | High | H21/H51 | optical measurement required | Mitigating |
| R-021 | 창 4개가 서로 다른 Android process라고 오해 | Medium | Medium | docs/UX | task/Activity와 process 구분 | Mitigating |
| R-022 | Hub 종료가 shared session을 죽여 stream 창도 종료 | Medium | High | H05/H29 | process-owned Rust core/lease, lifecycle tests | Open |
| R-023 | malformed media가 native crash를 유발 | Medium | Critical | H07/H44 | caps/fuzz/unsafe review/watchdog | Open |
| R-024 | display refresh/compositor가 60fps frame을 실제로 보여 주지 않음 | Medium | High | H08/H21 | Surface/refresh trace, profile 조정 | Open |

## 3. R-001 상세: multi-instance

### 가설

Android XR 공식 사용자 안내는 같은 앱의 여러 창을 지원한다. Android 15 property와 document task API로 Leftcar가 4개 stream window를 만들 수 있다.

### 아직 모르는 것

- Galaxy XR OS가 앱이 programmatic하게 만든 새 task를 어떻게 배치하는가
- 사용자가 “New Window” System UI affordance를 통해서만 만들 수 있는지
- task 상한과 background visibility
- RN Activity root 재사용

### 판정

Galaxy XR 실기기에서 네 창을 10분 유지하고 task dump와 visible counter를 남긴다. emulator만으로 닫지 않는다.

## 4. R-003 상세: decoder budget

Android `getMaxSupportedInstances()`는 upper-bound hint다. performance point도 single codec 가정이 포함된다. Samsung의 8K60 재생 사양은 네 decoder 동시 보장이 아니다.

제품 fallback 우선순위:

1. focus 1440p60 + background 720p15 x3
2. 모두 1080p30
3. focus 1080p60 + background 540p15
4. visible window 수에 따라 suspend
5. 동시 창 수 제한

사용자가 창을 열 수 있는데 영상이 검게 되는 것보다, 열기 전에 자원 부족과 권장 action을 보여 준다.

## 5. R-004/R-006 상세: TS/Rust 중심

팀의 주 언어는 TypeScript와 Rust다. 그러나 Android component는 manifest와 Activity가 필요하고 React Native native component는 platform implementation이 필요하다.

허용 타협:

- Kotlin은 thin shim
- TS Codegen spec이 public API source of truth
- decoder/network/session은 Rust
- UI는 TS

금지 타협:

- Kotlin ViewModel에 session state
- Kotlin coroutine network loop
- Kotlin MediaCodec quality policy
- TS로 frame bytes 전달
- Android 문제를 숨기기 위한 무검증 third-party binary

## 6. R-007 상세: transport

### QUIC 실패 신호

- loss에서 frame recovery 불안정
- 자체 congestion/pacing이 network를 과점
- packetization/fuzz 범위가 과도
- Android background/reconnect 문제

### WebRTC 실패 신호

- buffer를 낮출 수 없어 latency creep
- custom encoded source/decoder path integration 과도
- 4 track resource가 과도
- reproducible build/package 불가

선택 후 loser를 계속 유지하는 비용도 위험이다. bake-off 이후 product build는 한 후보만 기본으로 한다.

## 7. R-008/R-012 상세: 앱 창 source

source identity를 제목/위치 heuristic으로 자동 재연결하면 잘못된 문서를 보여 줄 수 있다. 사용자가 승인한 native source reference가 사라지면 `source_unavailable`로 만들고 재선택을 요구한다.

실제 가상 디스플레이는 이 문제를 일부 해결하지만 큰 새 프로젝트다. 다음 evidence 전에는 시작하지 않는다.

- 창 최소화/가림이 핵심 workflow를 반복적으로 막음
- display capture로 해결되지 않음
- 사용자가 OS-level 별도 desktop을 실제로 요구

## 8. 미결정 사항

| ID | 질문 | 결정 시점 | 필요한 증거 |
| --- | --- | --- | --- |
| Q-001 | 같은 source 중복 창을 허용할까 | G1 | decoder/resource와 UX |
| Q-002 | Hub를 닫아도 session을 얼마 동안 유지할까 | G1/G4 | lifecycle/battery |
| Q-003 | WebRTC vs QUIC | G2 | bake-off |
| Q-004 | network wire format | G2/P4 | fuzz/version/size |
| Q-005 | Android `FLAG_SECURE` 기본값 | G1/P8 | Home Space compatibility/privacy |
| Q-006 | H.264 profile/bitrate defaults | G3/G4 | text/latency/bandwidth |
| Q-007 | HEVC v1 포함 여부 | G4 | H.264 품질, codec budget |
| Q-008 | 동시 창 supported max | G4 | W4/W6 soak |
| Q-009 | background window suspend threshold | G4 | power/usability |
| Q-010 | macOS 최소 버전 | G3/package | ScreenCaptureKit features |
| Q-011 | Windows supported GPU/OS | G5 | device matrix |
| Q-012 | Play Store vs sideload/internal | P8 | distribution requirements |
| Q-013 | Overview mode 포함 | beta | user feedback |
| Q-014 | Linux 우선순위 | post-v1 | demand |

## 9. 위험 갱신 템플릿

```markdown
### R-NNN title

- 상태:
- 확률/영향:
- owner:
- last checked:
- new evidence:
- trigger:
- mitigation tried:
- next experiment:
- decision date:
```

## 10. 현재 권장 결정

조사만으로 권장할 수 있는 것:

- Home Space multi-instance를 먼저 시험한다.
- TS/Rust 중심과 얇은 Kotlin shim을 유지한다.
- Rustra는 control plane에 쓴다.
- 앱 창 캡처를 virtual display보다 먼저 한다.
- H.264를 baseline으로 한다.
- transport는 아직 고르지 않는다.
- 성능 수치는 실기기 전까지 목표로만 표시한다.

