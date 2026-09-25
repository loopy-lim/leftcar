# 검증 로그 — 폰트/디자인 개선 전체 최종 검증 (run loop 후속 verify 태스크)

- 실행일: 2026-02-14 (로컬 워크스테이션, 작업 트리 기준)
- 범위: (선행 run 완료분) site 폰트 적용 + desktop 감사·구현, (본 run 선행 의존) viewer-expo 감사·구현까지 포함한 전체 수용 확인
- 작성 주체: verify 워커 (최종 검증 전용 — 미달 항목은 수정하지 않고 기록)
- 판정: **전체 PASS** (아래 상세)

## 1. AGENTS.md 품질 게이트 (실측 실행)

| # | 게이트 | 명령 | 결과 |
|---|--------|------|------|
| G1 | React Doctor | `npx -y react-doctor@latest . --verbose` (루트) | **PASS — 100 / 100** "No issues found!" (176 파일 스캔: host-desktop, viewer-android, viewer-expo) |
| G2 | 루트 타입 체크 | `bun run typecheck` (루트) | **PASS** — check-react-runtime-versions(React 19.2.3 exact), `tsc -b`, viewer-expo / host-desktop `tsc --noEmit` 모두 오류 0 |
| G3 | site 빌드 | `bun run build` (cwd=site) | **PASS** — vite v6.4.3, 44 modules, `✓ built in 483ms` |
| G4 | 루트 테스트 | `bun run test` (루트) | **PASS** — Test Files 70 passed, Tests 852 passed (vitest v4.1.11) |

미검증 블로커: 없음. 4개 게이트 모두 실제 실행으로 확인했다.

## 2. 폰트 수용 확인

| # | 기준 | 확인 방법 | 결과 |
|---|------|-----------|------|
| F1 | woff2 서브셋 존재 | `site/public/fonts/` | **PASS** — `WantedSans-Bold.subset.woff2` + `OFL.txt`. 빌드 산출물 `site/dist/fonts/`에도 동일 포함 |
| F2 | @font-face + font-display: swap | `site/src/index.css:3-9` | **PASS** — `font-family: "Wanted Sans"`, weight 700, `font-display: swap`, `src: url("/fonts/WantedSans-Bold.subset.woff2")` self-host 상대경로. `site/index.html:9`에 `<link rel="preload" as="font" crossorigin>` 1건 |
| F3 | 외부 폰트 CDN 0건 | grep `fonts.googleapis/gstatic/bunny/typekit` on `site/src`, `site/index.html`, `site/dist/index.html` | **PASS** — 0건. CSP `font-src 'self'`와 정합 (`site/public/_headers:5`) |
| F4 | 라이선스 고지 | `site/LICENSE-fonts.md` | **PASS** — 존재 (OFL 원문은 `public/fonts/OFL.txt`) |
| F5 | design.md 폰트 항목 확정 표기 | `site/design.md:82, 180` | **PASS** — "디스플레이 = Wanted Sans Bold(700) 1웨이트 커스텀 서브셋 self-host, 본문 = 시스템 UI 스택 (FONT-DECISION.md §3.1 확정)" 명시. 서브셋 37.2KB(387자), 재생성 스크립트(`site/scripts/subset-font.sh`) 안내 포함 |

## 3. 양앱 수용 확인 (감사 문서 ↔ 구현 대조)

### 3.1 apps/viewer-expo (DESIGN-REVIEW.md 상위 5)

감사 문서 존재 확인: **PASS** — `apps/viewer-expo/DESIGN-REVIEW.md`가 `site/design.md` 탈AI 원칙 + `FONT-DECISION.md §3.3`(viewer는 시스템 폰트 유지, monospace 20건은 허용 예외) 기준으로 작성됨. 소스 `fontFamily` 선언은 monospace 외 0건으로 방침과 정합.

| # | 감사 항목 | 구현 확인 (git diff + 소스 실측) | 판정 |
|---|-----------|--------------------------------|------|
| 1 | 다크 저대비 + 미세 타이포 | `fontSize: 11` 잔존 0건(app+src 전수 grep). `sectionTitle`이 typography.lg(18) 600 + textPrimary로 위계 복원(hub-styles.ts:229-234). textDim의 텍스트 용도 제거 — 잔존 textDim은 backgroundColor(도트 2건), 아이콘 색, OTP 채움 전 상태뿐. **주의**: textMuted는 아이콘 색(chevron 등)·플레이스홀더에 일부 잔존 — 본문 텍스트 대비 기준은 충족하나 "textMuted→textSecondary 전면 치환"이라는 구현 요약 표현과는 부분 일치 | **PASS (부분 메모)** |
| 2 | 타이포 토큰 적용 | hub-styles.ts + app/pairing.tsx가 `typography.*` 소비 확인. catalog-styles 등 나머지 화면은 미적용 — 구현 요약도 "부분 구현"으로 자진 명시 | **부분 (문서화된 한계)** |
| 3 | 첫 화면 위계 (G2 카드 그리드) | FeatureCardsGrid.tsx 전면 재작성 확인 — 2열 균일 카드 → 정의형 행 2줄(제목 600 + 설명, 1px featureDivider), 아이콘 박스 제거 | **PASS** |
| 4 | 게이트·상태 신호 | 재시도 버튼 `retryBtn`/`retryText` 스타일 + `accessibilityRole="button"` 부여 확인(pairing.tsx:717-721, 870-881 — handleQrScanned 로직 미변경). hostDot·배지 강등은 diff로 확인 | **PASS** |
| 5 | maxWidth 캡 | hub/pairing 640(hub-styles.ts:20, pairing.tsx:757), catalog/host 840(catalog-styles.ts:20, host.tsx:842) + alignSelf center 확인 | **PASS** |

### 3.2 apps/host-desktop (DESIGN-REVIEW.md 상위 5)

감사 문서 존재 확인: **PASS** — design.md §1~§4 기준, 축별 이슈(A/D/T/X/C) + 우선순위 상위 5 + 구현 워커용 주의사항 포함.

| # | 감사 항목 | 구현 확인 | 판정 |
|---|-----------|-----------|------|
| 1 | X-1 화질 슬라이더 카드 본체 승격 | `QualityOverride.tsx` 신규 추출(§3의 props 동등 유지 지시 준수), SessionInspector에서 분리 | **PASS** |
| 2 | A-2+D-1 메트릭 그리드 붕괴 + 상태 4중 표기 정리 | SessionCard/StreamsListView diff로 확인 | **PASS** |
| 3 | T-1 Tailwind 팔레트 우회 제거 | `diagnosticStyles.ts`에서 `!text-sky-600`/`!text-amber-600` 0건 확인 | **PASS** |
| 4 | A-1 pill 강등 | `footer-tag-pill`/`host-version-badge`/`session-tag`/`live-badge` TSX 잔존 0건. `count-pill`은 mono 텍스트 스타일로 강등(보더·배경 없음, index.css:1597). `host-status-pill` radius 7px(감사안 6~8px 범위 내). `input-request-badge`(행동 배지 예외)·`countdown-badge`(QR 카운트다운)·`device-live-badge`는 감사 문서의 예외/비우선 항목으로 잔존 | **PASS** |
| 5 | X-2+X-3 실험 노브 강등 + 인스펙터 위계 | `ExperimentsSection.tsx`가 설정 모달 섹션(`settings-section-title`)으로 존재 확인. 정적 확인 수준이며 시각 렌더 검증은 범위 밖 | **PASS (정적 확인)** |

## 4. 기능 로직 미변경 확인 (git diff)

- `toggleGate.ts`, `hostState.ts`, `streamTermination.ts`, `paired-device-state.ts`, `rustra-bridge` 관련: **변경 0건** (git diff --name-only 전수 대조)
- 변경된 테스트 1건과 사유: `apps/host-desktop/src/encoderDiagnostics.test.ts` — X-1에 따라 QualityOverride가 SessionInspector 밖으로 승격됨에 따라 렌더 대상을 분리한 것. 기존 기능 어설션("40% 고정 · Base QP 기반" 등)은 유지되며 감사 문서 §3이 사전 예고한 변경. 기능 로직 파일(encoderDiagnostics.ts本体)은 미변경.
- 신규 파일 1건: `apps/host-desktop/src/QualityOverride.tsx` — UI 추출 전용.

## 5. 미해결 리스크 / 한계

1. viewer-expo #2 타이포 토큰화는 hub/pairing에 한정 — catalog/host 화면 전면 토큰화는 차기 라운드 과제로 남음(구현 요약도 동일 명시).
2. textMuted가 아이콘 색·플레이스홀더에 잔존 — 본문 텍스트 기준은 충족하나 후속 run에서 의도 잔존인지 재확인 권장.
3. 대비 수치·실기기 렌더링은 정적 검증 범위 밖(감사 문서도 동일 한계 명시).
4. 배포·Cloudflare 트리거 작업은 제약에 따라 수행하지 않았음.

## 6. 종합 판정

**PASS** — 품질 게이트 4종(react-doctor 100/100, typecheck, site build, 루트 테스트 852건) 전부 통과, 폰트 수용 기준 5종 전부 통과, 양앱 감사 문서와 구현의 대응 확인, 기능 로직 미변경 확인. 위 §5의 한계는 실패 사유가 아닌 후속 권고로 기록한다.

---

# 부록 — 이전 검증 로그 (site 랜딩 사이트 분, 참고 보존)

# 검증 로그 — 탈AI 디자인 개선 + 보안 하드닝 (verify 태스크)

- 실행일: 2026-02-14 (로컬 워크스테이션, 작업 트리 기준)
- 범위: site/ 랜딩 사이트의 IMPROVEMENT-PLAN.md / AI-LOOK-REVIEW.md / SECURITY-REVIEW.md 반영 결과 검증
- 작성 주체: verify 워커 (최종 검증 전용 — 미달 항목은 수정하지 않고 기록)

## 1. AGENTS.md 품질 게이트

| # | 게이트 | 명령 | 결과 |
|---|--------|------|------|
| G1 | site 빌드 | `bun run build` (cwd=site) | **PASS** — vite v6.4.3, 44 modules transformed, `✓ built in 458ms` |
| G2 | React Doctor | `npx -y react-doctor@latest . --verbose` (저장소 루트) | **PASS — 100 / 100** "No issues found!" (스캔 175 파일, 프로젝트: host-desktop, viewer-android, viewer-expo) |
| G3 | 타입 체크 | `bun run typecheck` (저장소 루트) | **PASS** — check-react-runtime-versions 통과(React 19.2.3 exact), `tsc -b` + viewer-expo / host-desktop `tsc --noEmit` 오류 없음 |

도구 실행 실패·미검증 블로커: 없음. 세 게이트 모두 실제 실행으로 확인했다.

## 2. 수용 기준 재확인

| # | 기준 | 확인 방법 | 결과 |
|---|------|-----------|------|
| A1 | dist에 보안 메타/헤더 파일 포함 | `site/dist` 목록 및 내용 검사 | **PASS** — `dist/_headers` 존재(CSP·nosniff·DENY·Referrer-Policy·Permissions-Policy·COOP·HSTS 7종), `dist/favicon.svg`, `dist/og-image.png` 포함. `dist/index.html`에 `favicon` link, `canonical`, og:title/description/url/type/image(+width/height) 메타 존재 |
| A2 | site.ts에서 SITE_VERSION 참조 정상 | grep `site/src` | **PASS** — `site/src/content/site.ts:26`에서 `export const SITE_VERSION = 'v0.1.4'` 정의, badge(30행), CTA URL(35행), status.lead(95행) 3곳이 상수 참조. 버전 하드코딩 잔존 없음 |
| A3 | 'Rustra는' 오타 잔존 0건 | grep `Rustra는` | **PASS** — 0건. Rustra 언급은 `site.ts:92`에 1건 유지되며 "명령·상태·오류 경계는 Rustra가 관리하는 계약으로 정의하고…"로 도구 역할이 드러나게 재작성됨 (의도 계약 유지 확인) |
| A4 | 알고리즘명(ChaCha20/Ed25519) site/src 잔존 0건 | grep -iE `chacha20\|ed25519` | **PASS** — 0건 |
| A5 | pill 배지 잔존 0건 | grep `pill` (tsx/css) | **PASS** — pill 셀렉터·클래스·마크업 0건. `Hero.css:8`에 "사실 나열은 pill 없이 텍스트 한 줄 (design.md G1)" 주석만 존재(제거 근거 문서화용, 렌더링 요소 아님) |
| A6 | Security/Status center 정렬 잔존 0건 | grep `text-align/align-items/justify-content` on Security.css, Status.css | **PASS** — 두 파일 모두 섹션 스코프에서 `text-align: left` 오버라이드. center 정렬 선언 0건 (Status.css:53-54의 `align-items: baseline` / `justify-content: space-between`은 그리드 행 배치용이며 텍스트 center 정렬 아님) |
| A7 | spacing px 잔존 0건 (1px 보더 제외) | grep `(padding\|margin\|gap)[^:;]*:.*[0-9]+px` on site/src CSS | **PASS** — 0건. 남아 있는 px는 spacing이 아닌 항목뿐: 레이아웃 `max-width`(768/1024px), `border-radius`(8/12px), 미디어쿼리 중단점(768px) — 디자인 토큰 정책상 허용 범위 |

## 3. 종합 판정

**전체 PASS.** 품질 게이트 3항목(빌드·react-doctor 100/100·typecheck)과 수용 기준 7항목(A1~A7)이 모두 통과했다.

## 4. 범위 밖 / 미해결 항목

- P3(아이덴티티 폰트 도입, 팔레트 교체): 의도적으로 이번 라운드에서 제외됨 (서체 선정이라는 별도 결정 필요). 미착수 상태이며 블로커 아님.
- `_headers`의 HSTS `includeSubDomains` 제외는 보안 하드닝 워커가 남긴 의도적 결정(동일 계정 타 서브도메인 영향 확인 전 유보). 배포 후 CSP가 실제 응답에 적용되는지 Cloudflare Pages 응답 헤더 확인은 배포 작업 범위이므로 이번 검증에서 미수행.
- React Doctor 진단 원문: `/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/react-doctor-434b247c-eda2-441a-b6fb-da9574c40015`
