# 뷰어 주도 화면 크기 검증 기록

작성일: 2026-09-06. 실기 측정: 2026-09-06 (Lenovo TB710FU).
상태: 코드·자동 검사 완료. 태블릿 자동 매칭·스트림 실측 완료, Galaxy XR·cgvd-shim RESIZE·BetterDisplay 미수행.
설계: docs/plans/2026-09-06-viewer-display-sizing-design.md (사용자 승인 2026-09-06)
계획: docs/plans/2026-09-06-viewer-display-sizing.md

## 구현 요약 (커밋 순)

| 커밋 | 내용 |
| --- | --- |
| 291dbb3 | 설계 문서 |
| ebd9428 | 구현 계획 |
| f544710 | 자동 매칭 순수 함수 (match_display_size) |
| 9e1d4a3 | 계약 확장 — viewerDisplay/virtualDisplayId 필드, resizeVirtualDisplay 명령 |
| 2349272 | cgvd-shim RESIZE stdin 프로토콜 |
| 096d73a | 뷰어 getDisplayMetrics 네이티브 모듈 + 시작 요청 전달 |
| 7138b38 | 태블릿 가상 화면 크기 카드 UI (프리셋·직접 입력·실시간 적용) |
| 7d26c3a | 매칭 공식 정정 — 물리 px 1:1 backing 기준 (계획문 dpi 공식은 고밀도 태블릿에서 실패) |
| bcf8de2 | XR 창 비율 프리셋 (16:10/16:9/4:3/9:16, SpatialWindow 재적용) |
| 925f1cf | DisplayManager.resize — CGVD in-place 모드 재적용, BetterDisplay는 안내 에러 |
| f50e487 | 스트림 시작 시 자동 매칭 연결 — 재사용/리사이즈 best-effort, 자동 생성은 안전상 생략 |

## 자동 검사 결과 (2026-09-06 최종)

- React Doctor: 100/100
- `bun run typecheck`: 통과
- `bun run test`: 34파일 389테스트 통과
- `bun run test:architecture`: TS/Kotlin 규칙 클린
- host-desktop `cargo test`: 128 lib + 10 e2e 통과 (무시 2개는 실기 필요 항목)
- `cargo test -p control-contract`: 14 + 18 통과
- cgvd-shim `swift build`: 성공
- Android `:app:testDebugUnitTest` + `:app:compileDebugKotlin`: exit 0 (전체 23테스트 통과)

## 설계에서 조정된 사항

1. **매칭 공식**: 계획문의 dpi 기반 공식(물리 ÷ (dpi/160) ÷ 2)은 420dpi 태블릿 2800×1752에서
   논리 후보가 최소 미달이 되어 항상 None. 승인 설계의 의도(backing 픽셀을 태블릿 물리 픽셀과
   1:1)로 정정 — 논리 = 물리 ÷ 2, scale 2 우선, 1280×720 미달 시 scale 1 폴백. `density_dpi`
   필드는 계약 호환용으로 유지하되 산출에 사용하지 않음.
2. **자동 생성 생략**: 설계 §1의 "일치하는 관리 화면이 없으면 자동 생성"은 구현하지 않음.
   호스트 UI 토글 상태가 제어 평면에 노출되지 않고, 단일 세션 정리 정책과의 충돌 방지를 위해
   화면 생성 소유권은 호스트 UI(DisplayManagerCard)의 명시적 사용자 동작으로 남김. 시작 경로는
   관리 화면 재사용·리사이즈만 수행(best-effort, 실패해도 스트림은 계속).
3. **BetterDisplay 리사이즈**: CLI에 라이브 모드 전환 동사가 없어 제거→재생성은 화면 ID/UUID
   변경과 소유권 공백 위험이 있음. 명확한 안내 에러를 반환하도록 하고 재생성은 UI 경로에 맡김.

## 미검증 항목 (실기 필요 — 완료 처리하지 않음)

태블릿(Lenovo TB710FU, 192.168.0.19) — 실기 측정 2026-09-06:
- [x] 연결 시 시작 요청의 viewerDisplay 메트릭 실측과 자동 매칭 결과 확인
      — 가로 3200×2000, density 400 → 자동 매칭 1600×1000@2x. 승인 공식(물리 ÷ 2,
      scale 2)과 정확히 일치. 카탈로그에 내장 디스플레이만 있어(1개) LogOnly 분기,
      관리 화면 생성은 호스트 UI 전용으로 남음 (호스트 stdout 9행).
- [ ] 관리 화면 재사용/리사이즈 경로가 실제 CGVD 화면에서 도달하는지
      — 관리 화면이 존재하지 않아 경로 미도달 (측정된 실제 결과, 검증 아님)
- [ ] 크기 카드에서 프리셋 전환 시 스트림 재시작 없이 해상도 전환되는지와 전환 지연
- [ ] 직접 입력 범위(640×480~4096×4096) 외 값의 오류 표시와 기존 크기 유지
- [ ] 180초/10분 관점의 전환 후 안정성(프레임 복구 포함)

Galaxy XR:
- [ ] 비율 프리셋(16:10/16:9/4:3/9:16) 전환 시 SpatialWindow 비율 실시간 변화
- [ ] 비율 전환 후 Mac 가상 화면 해상도 불변 확인
- [ ] config-change/프로세스 사망 후 비율 복원
- [ ] 9:16 세로 창과 홈 스페이스 패널 bounds 충돌 여부

cgvd-shim RESIZE (GUI 세션 필요):
- [ ] create → RESIZE 실제 모드 전환 도달과 소요 시간
- [ ] 생성 크기보다 큰 RESIZE가 descriptor.maxPixels로 거절될 때 FAILED 응답 정상 반환
- [ ] scale 1↔2 왕복 전환
- [ ] RESIZE 실패 후 프로세스 생존과 후속 명령 처리
- [ ] RESIZE 직후 PLACE가 새 논리 bounds로 성공

BetterDisplay:
- [ ] 리사이즈 시도 시 안내 에러 노출 확인 (호스트 UI 연결은 후속)

## 실기 측정 결과 (2026-09-06, Lenovo TB710FU)

- 뷰어 연결: 태블릿 192.168.0.19 → 호스트 192.168.0.134 (제어 :7777, 미디어 :5001).
  화면 선택 UI에 연결된 컴퓨터·Display 0·3840×2160·60 FPS·"동영상 우선" 표시 확인.
- 자동 매칭: 시작 요청의 viewerDisplay(3200×2000, density 400) → 1600×1000@2x 매칭.
  관리 화면 부재로 LogOnly 분기 (stdout: "viewer display match: no managed display to
  prepare … matched size 1600x1000@2x logged only"). 카탈로그 warm: 1 display(s).
- 스트림: stderr `Leftcar first capture frame 192.168.0.19:5001: 3840x2160` (23:52:57),
  `first split media pair sent … bytes=345361`, route=split, split recovery pairs
  (bytes=243682/466170/442400/235896). H264 적응 비트레이트 48M→38.4M→30.7M→24.6M→
  19.7M→15.7M→14M 하한 도달 후 회복(15.7M/18.2M), congested=true 구간 정상 동작,
  14M 하한에서 resolution fallback 요청 반복.
- 뷰어 화면 전환: `topResumedActivity=…leftcar.ll3.kr/dev.leftcar.viewer.stream.StreamActivity
  t46` 확인 (2회, 안정). best-effort 정책대로 스트림은 정상 시작.
- 스크린샷 /tmp/lc_stream.png(3200×2000)은 캡처 성공했으나 Mac이 GUI 세션 밖 상태라
  데스크톱 콘텐츠 육안 확인은 불가 — 프레임 송수신은 호스트 로그로 입증.

## 알려진 이슈

1. 호스트에 화면 녹화 권한 미부여 → "display catalog warmup deferred" 경고.
   캡처는 shim dylib(libleftcar_capture.dylib)로 동작하므로 스트림에는 영향 없음.
2. DMG 번들 실패: bundle_dmg.sh의 AppleScript Finder 장식 단계가 GUI 세션 밖
   osascript 실행에서 실패 (자동화 셸은 GUI 세션 밖 — Aerospace 환경 특성).
   .app 번들+서명(identity 3D28D078…)은 성공, 공증은 환경변수 미설정으로 skip.
   오류: `failed to bundle project: error running bundle_dmg.sh: \`failed to run
   …/bundle/dmg/bundle_dmg.sh\``
3. AOAP `claim USB control interface failed: could not be opened for exclusive access`
   노이즈 — 기존 존재, 본 검증과 무관.
