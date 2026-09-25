# DESIGN-AUDIT §4 수용 기준 검증 (AUDIT-VERIFICATION)

- 검증 일시: 2026-09-26 (워킹트리 미커밋 상태 기준)
- 검증 대상: `site/DESIGN-AUDIT.md` §4 수용 기준 7항목 + §5 미해결 리스크 3건
- 검증 방법: 워킹트리 코드 대조(정적 검증) + `bun run build` + `react-doctor` 2회(루트/site 스코프)

---

## 1. §4 수용 기준별 판정

### ① 폰트 크기 7단계 토큰 안, px 폰트 선언 없음 — **PASS**
- 전체 CSS에서 `font-size` 17건이 모두 `var(--text-*)` 토큰 사용 (grep `font-size` 결과: index.css 2, Card 2, Hero 3, StackBadges 3, Security 2, Diagram 1, Status 3, Footer 1). px 단위 font-size 0건.
- 토큰 정의는 `src/index.css:12-20` — §2.1 표와 값 일치(`--text-hero` clamp(2.25rem,5vw,3.25rem) 등 7단계).
- h1은 히어로 1개뿐(`Hero.tsx:10`), 섹션당 h2 1개(FeatureGrid "주요 기능" / Security "보안" / Status "지원 현황"), 카드·행 제목 h3.
- ⚠️ 경미한 §2.2 편차(기준 ① 자체는 아님): `Diagram.css:91` `padding: 10px`, `Diagram.css:94` `gap: 8px` — 도식 장식 요소의 px 간격 2건. §2.2 "px 직접 사용 금지"에는 위배되나 §4 기준 문구(폰트 크기) 범위 밖.

### ② 섹션 세로 패딩 `--section-y` 균일, 중첩 margin 없음 — **PASS**
- 공통 정의 `src/index.css:60-63` `.section-container { padding: var(--section-y) var(--space-4) }` 단일화. FeatureGrid(`section-container`)·Security(밴드 내부 `section-container`)·Status(`section-container`)가 공용.
- 이전 중첩 margin 제거 확인: Diagram `margin: var(--space-6) 0 0`(`Diagram.css:8`, 구 4rem 0 → 상단만), Footer `margin-top` 없음 + `padding: var(--space-7) var(--space-4)`(`Footer.css:7`), Security 리드 `margin: var(--space-3) auto 0`(공통).
- StackBadges는 `--section-y / 2` 스트립(`StackBadges.css:6-7`) — §3-10이 명시 허용한 예외.
- Hero는 헤더로 `padding: var(--section-y) var(--space-4) 0`(`Hero.css:2`) — 섹션 컨테이너 비적용이지만 상단 리듬은 `--section-y` 유지.

### ③ 카드 크롬은 Features 4개뿐, Security/Status는 행 기반 — **PASS**
- `Card` 사용처: `FeatureGrid.tsx`뿐(카드 4개). `Security.tsx`는 `ul.security-list` 2열 리스트 행(`Security.css:16-28`, 카드 크롤 없음), full-bleed `security-band`(`Security.css:2-6`).
- `Status.tsx`는 `dl.status-list` 정의형 행(좌 `dt` / 우 `dd` 라벨, `--border` 구분선). 고아 카드 문제(C7) 해소.
- §5-3 리스크(밴드 full-bleed 모바일 패딩): 밴드 자체는 좌우 패딩 없고 내부 `.section-container`가 `var(--space-4)` 좌우 패딩 제공 → 구조상 해소. (시각 확인은 미수행 — 정적 구조 판정)

### ④ accent 사용처 3종 이하 — **PASS**
- ① primary CTA 배경(`Hero.css` `.primary-cta`), ② 본문 링크(`index.css:44` `a { color: var(--accent) }`), ③ Status 하이라이트 1곳(`Status.css:30-33`). 정확히 3종.
- 다이어그램 선·라벨·화살표는 전부 `--text-dim`(`Diagram.css:45,54,60`), 히어로 배지·스택 배지는 중립(`--surface`+`--border`+`--text-dim`). 신규 색상 토큰 없음(6토큰 유지).

### ⑤ 하이라이트 상태 라벨 1개 — **PASS**
- `site.ts`에서 `highlight: true` 1건(`st1` macOS Host, site.ts:107). 나머지 4개는 `status-normal`(`--text-dim`).

### ⑥ 정보 손실 없음 + §3 정리 지시 이행 — **부분 충족 (1건 보류)**
- 정보 보존: 기능 4 / 보안 4 / 스택 배지 5 / 상태 5 / 푸터 링크 4 — 모두 유지 확인(`site.ts`).
- subcopy 2문장 축소 ✓(site.ts:35), `status.lead`에서 APK 파일명 제거 ✓(site.ts:116, "상태 기준일 2026-09-13 · 최신 릴리스 v0.1.4").
- **미이행**: `stack.description`의 "Rustra는" 오타 **그대로 존재**(`site.ts:90`). §3-11이 "구현 전 사용자 확인"을 조건으로 했으므로 구현이 아니라 **확인 대기 보류** 상태. → §5-1 리스크 잔존.

### ⑦ 빌드 성공 + react-doctor 100/100 — **PASS (단, 게이트 범위 주의 필요)**
- `cd site && bun install && bun run build`: **성공**. `bun install v1.4.1 — 126 installs across 221 packages (no changes)` / `vite v6.4.3 — ✓ 46 modules transformed, ✓ built in 461ms`, 산출물 dist/index.html 0.73kB + CSS 6.88kB + JS 202.34kB. 오류/경고 없음.
- `npx -y react-doctor@latest . --verbose` (저장소 루트, AGENTS.md 게이트대로): **100/100, No issues found** (175 파일 스캔).
- ⚠️ **게이트 범위 유의**: 루트 `package.json` workspaces는 `packages/*`, `apps/*`뿐이라 루트 실행 시 스캔 프로젝트가 `@leftcar/host-desktop, @leftcar/viewer-android, @leftcar/viewer-expo` 3개이며 **`site/`(leftcar-site)은 미포함**. 이에 `cd site && npx -y react-doctor@latest . --verbose` 추가 실행 → **leftcar-site 100/100, 14 파일 스캔, No issues found**. 두 실행 모두 통과이므로 게이트는 실질 충족이나, 루트 실행만으로는 site를 검증하지 못한다는 점을 기록으로 남김.

---

## 2. §5 미해결 리스크 현재 상태

| 리스크 | 상태 | 근거 |
|---|---|---|
| "Rustra는" 오타 원 의도 단어 미확정 | **여전히 미해결** — 오타 미수정 (`site.ts:90`) | 사용자 확인 대기. 문맥상 "Tauri는" 추정이나 확정 없음 |
| 섹션 제목 한국어 문구 / Features 리드 문구 확정 | **제안 문구로 구현됨, 공식 확정 대기** | "주요 기능"/"보안"/"지원 현황", 리드 "설치된 PC의 화면을 승인된 Android 기기에서 여러 창으로 확인합니다."(`site.ts:59`) — 모두 감사 문서 제안안 그대로 |
| Security 밴드 full-bleed 반응형 | **구조상 해소, 시각 검증 미수행** | 밴드는 패딩 없는 full-bleed, 내부 `section-container`가 좌우 패딩 담당(`Security.css:2-6`, `index.css:60-63`) |

---

## 3. 검증 실행 기록

| 체크 | 위치 | 결과 |
|---|---|---|
| `bun install && bun run build` | `site/` | 성공 (vite 6.4.3, 46 modules, 461ms, 오류 0) |
| `npx -y react-doctor@latest . --verbose` | 저장소 루트 | 100/100 — 단, site 미스캔(워크스페이스 불포함) |
| `npx -y react-doctor@latest . --verbose` | `site/` | 100/100 (leftcar-site, 14 파일) |
| 정적 grep: font-size px / highlight 수 / Card :hover / `html{lang:ko}` | `site/src/` | px 폰트 0건 · highlight 1건 · `.card:hover` 제거 확인 · 죽은 코드 제거 확인(`index.html:2`에 `lang="ko"` 존재) |

---

## 4. 종합 판정

**§4 수용 기준 7항목 중 6항목 PASS, 1항목(⑥) 부분 충족.** 빌드·품질 게이트는 모두 통과(사실 기반). 다만 아래 미충족·보류 항목이 남는다.

### 미충족 / 보류 항목
1. **"Rustra는" 오타 미수정** (`site/src/content/site.ts:90`) — §3-11·§5-1. 원 의도 단어(추정: "Tauri는") 사용자 확정 전까지 보류. 유일한 §4-⑥ 미이행.
2. **한국어 섹션 제목·Features 리드 문구 미확정** — 현재 감사 문서 제안안이 그대로 코드에 반영됨. 카피 확정 전까지 "제안 상태"로 기록 유지 권장.
3. **(경미) `Diagram.css:91,94` px 간격 2건** — §2.2 스페이싱 토큰 규칙 위반(장식 도식 요소). §4 기준 자체는 아니지만 다음 손질 시 `--space-2`/`--space-1`로 치환 권장.
4. **(프로세스) 루트 react-doctor 게이트가 site/를 스캔하지 않음** — 루트 workspaces에 `site` 불포함. site 변경 시 `site/` 스코프 재실행이 필요하며, 이를 CI/게이트 문서에 명시할 것을 권장.
