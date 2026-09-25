# Leftcar 랜딩 시각 혼잡도 감사 (DESIGN-AUDIT)

- 범위: `site/src/**` (App, Hero, FeatureGrid, Security, StackBadges, Status, Footer, Diagram, Card, index.css, content/site.ts)
- 성격: 의사결정 문서 — 후속 구현 태스크가 이 문서의 지시를 그대로 실행한다.
- 판정 기호: **유지 / 축소 / 제거 / 통합 / 재구조화 / 수정**

---

## 1. 혼잡 원인 진단 (근거 기반, 11건)

### C1. 동일한 2열 카드 그리드가 3연속 — 페이지 최대 혼잡원
- 근거: `FeatureGrid.css` `.features-grid`, `Security.css` `.security-grid`, `Status.css` `.status-grid`가 모두 `grid-template-columns: repeat(2, 1fr)` + `gap: 1.5rem`로 복붙 수준으로 동일하고, 세 섹션이 전부 `Card` 컴포넌트(`Card.css`: surface 배경 + 보더 + radius 12px)를 재사용한다. 결과적으로 페이지 중앙에 **표면·크기·형태가 같은 카드 13개**(4+4+5)가 연속 배치된다.
- 영향: "무엇을 하는가(Features) / 얼마나 안전한가(Security) / 어디까지 되었나(Status)"라는 서로 다른 질문이 시각적으로 구분되지 않고, 카드의 반복 자체가 노이즈가 된다.
- 판정: **재구조화** — Features만 카드 유지, Security는 틴트 밴드+리스트 행, Status는 정의형 행 리스트. (P0)

### C2. FeatureGrid에 가시 섹션 제목 없음
- 근거: `FeatureGrid.tsx`의 `<h2 className="sr-only">Features</h2>` — 화면에서 보이지 않는다. 히어로 다이어그램 바로 아래에 제목 없는 카드 4개가 뜬다. 문서 아웃라인상 h2 없이 h3만 노출되는 위계 결함이기도 하다.
- 판정: **수정** — 가시 제목 + 리드 문장 추가. (P0)

### C3. 타이포그래피 스케일 부재 + 공통 스타일의 암묵적 공유
- 근거: 폰트 크기가 파일마다 제각각 — `Hero.css` 0.85rem/clamp(2rem,5vw,3.5rem)/clamp(1rem,2vw,1.25rem), `Card.css` 1.2rem/1rem, `Security.css` 2rem/1.1rem, `StackBadges.css` 0.9rem, `Status.css` 0.9rem. **서로 다른 크기 8수준 + clamp 2종**이 혼재한다. 특히 `.section-title`/`.section-lead`는 `Security.css`에 정의되어 있는데 `StackBadges`/`Status`가 CSS 전역성에 의존해 암묵적으로 빌려 쓰는 중(컴포넌트 경계 위반).
- 판정: **통합** — 토큰화 + 공통 섹션 스타일의 `index.css` 일원화. (P0)

### C4. 히어로 과적
- 근거: `Hero.tsx`가 배지 2개(`.hero-badges`) + 헤드라인 + 3문장 subcopy(`site.ts` hero.subcopy) + CTA 2개 + 대형 다이어그램까지 모두 담는다. `Diagram.css` `.diagram-container`는 `margin: 4rem 0` + `padding: 4rem 2rem`(데스크톱) + surface 박스 + radius 16px로 히어로 내부에 또 하나의 "섹션"을 만든다. 헤드라인→CTA 사이에 스크롤 없이 소화해야 할 정보량이 과다하다.
- 판정: **축소** — 배지 병합, subcopy 축소, 다이어그램 무게 경량화. (P1)

### C5. 수직 리듬(스페이싱) 무질서
- 근거: 모든 섹션이 `.section-container { padding: 4rem 1rem }`인데, `Diagram.css`의 `margin: 4rem 0`, `Status.css`의 `margin-bottom: 2rem`, `Footer.css`의 `margin-top: 4rem` + `padding: 3rem`, `Security.css` 리드의 `margin-bottom: 3rem`, 배지들의 `2rem`/`1.5rem`/`0.5rem`이 중첩된다. 다이어그램→Features 사이는 실효 8rem, Security 리드→카드는 3rem 등 **같은 위계의 간격이 위치마다 2배 이상 차이**난다. 단위도 `12px`/`24px`(`Card.css`)와 rem이 혼용된다.
- 판정: **통합** — 스페이싱 토큰 + 섹션 패딩 단일화, 중첩 margin 제거. (P0)

### C6. 강조색(accent) 과다·의미 불일치
- 근거: accent가 5곳에서 서로 다른 역할로 사용 — `Hero.css` `.version-badge`(배경+보더+텍스트 틴트), `index.css` 링크, `.primary-cta` 배경, `Diagram.css`(네트워크 선/라벨/화살표), `Status.css` `.status-highlight`. 특히 Status에서 **진행 중/예정 항목 5개 중 4개가 accent(green)로 하이라이트**되어 "초록=사용 가능"이라는 관습과 어긋나고, 다수 하이라이트는 하이라이트가 아니다.
- 판정: **축소** — accent 사용처를 3종으로 제한, 상태 강조는 1곳만. (P1)

### C7. 표/리스트 성격의 데이터를 카드로 표현 (Status)
- 근거: `Status.tsx`는 "항목 → 상태"라는 2차원 데이터를 카드 5개로 렌더링한다. 2열 그리드에 5개라 마지막 행에 **고아 카드 1개**가 생기고, 카드 크롬(배경·보더·패딩 24px)이 데이터 대비 과하다. `site.ts` status.lead는 기준일·버전·APK 파일명 3개 사실을 한 문장에 욱여넣으며, APK 파일명은 히어로 CTA 라벨("v0.1.4 Viewer APK 받기")과 중복된다.
- 판정: **재구조화(축소 병행)** — 정의형 행 리스트로 전환, lead 축소. (P1)

### C8. StackBadges가 전체 섹션을 차지
- 근거: 배지 5개 + 설명 1문장인데 `section-container` 패딩 4rem + `.section-title`(2rem)을 소비한다. 정보 밀도 대비 수직 공간 낭비가 크고, `site.ts` stack.description에는 오타("Rustra는")가 있다.
- 판정: **축소** — 제목 없는 컴팩트 스트립으로 격하. (P2)

### C9. 섹션 제목 언어·의미 불일치
- 근거: 제목은 영어 단어("Security", "Tech Stack", "Status", sr-only "Features"), 본문은 전부 한국어. 단일 영어 단어 제목은 메시지를 전달하지 못해 본문 카드만 남겨 위계를 약화시킨다.
- 판정: **수정** — 한국어 서술형 제목으로 교체. (P2)

### C10. 비인터랙티브 요소의 hover 피드백
- 근거: `Card.css` `.card:hover { border-color: var(--text-dim) }` — 클릭할 수 없는 카드에 인터랙션 힌트를 준다. 마이크로 노이즈이며 오조작 유발.
- 판정: **제거**. (P2)

### C11. (위생) 유효하지 않은 CSS 선언
- 근거: `index.css`의 `html { lang: ko; }` — `lang`은 CSS 속성이 아니므로 무시되는 죽은 코드. 문서 언어는 HTML(`index.html`)에서 관리해야 한다.
- 판정: **제거**. (P2)

---

## 2. 디자인 방향 (후속 구현이 따를 기준)

### 2.1 타이포그래피 위계 — 7단계 스케일만 사용
`index.css`에 토큰으로 정의하고, 그 외 크기·clamp 직접 사용 금지. 단위는 rem만.

| 토큰 | 값 | 용도 |
|---|---|---|
| `--text-hero` | `clamp(2.25rem, 5vw, 3.25rem)` / 700 / lh 1.15 | 히어로 헤드라인 (유일) |
| `--text-2xl` | `clamp(1.5rem, 3vw, 2rem)` / 700 | 섹션 제목 |
| `--text-xl` | `1.375rem` / 600 | (예비, 사용 자제) |
| `--text-lg` | `1.125rem` / 400 / `--text-dim` | 섹션 리드 |
| `--text-md` | `1rem` / 400 | 본문, 카드 설명 |
| `--text-sm` | `0.875rem` / 500~600 | 카드 제목, 배지, 상태 라벨 |
| `--text-xs` | `0.8125rem` | 보조 노트, 푸터 저작권 |

규칙: 페이지에서 **h1은 1개(히어로)**, h2는 섹션당 1개, h3는 카드/행 제목. 히어로 헤드라인과 섹션 제목 사이에는 다른 크기를 넣지 않는다(위계 단순화).

### 2.2 섹션 간 리듬과 여백 스케일
- 스페이싱 토큰(rem): `--space-1: 0.25` `--space-2: 0.5` `--space-3: 0.75` `--space-4: 1` `--space-5: 1.5` `--space-6: 2` `--space-7: 3` `--space-8: 4`. px 직접 사용 금지.
- **섹션 세로 패딩은 `--section-y: clamp(3rem, 7vw, 4.5rem)` 하나로 통일.** 섹션 간 추가 margin 금지(다이어그램 `margin: 4rem 0`, 푸터 `margin-top: 4rem` 등 중첩원 제거).
- 컨테이너: `max-width: 1024px` 유지, 좌우 패딩 `--space-4`.
- 리듬 변화는 **배경 밴드로만**: Security 섹션만 full-bleed `--surface` 배경 + 상하 1px 보더를 주어 3연속 동일 그리드의 단조로움을 끊는다. 나머지 섹션은 배경 없음.
- 카드 내부: 패딩 `--space-5`, 제목-본문 간격 `--space-3`.
- 관련 요소 그룹 간격(제목→리드→본문): `--space-3` / `--space-6` 순으로 축소 계단.

### 2.3 색상 사용 규칙
- accent 사용처를 3종으로 제한: ① primary CTA 배경 ② 본문 링크 ③ Status의 "사용 가능" 신호 1곳.
- 신규 색상 토큰 추가 금지(기존 6토큰 유지). 배지는 전부 중립(`--surface`+`--border`+`--text-dim`).
- 다이어그램 내 accent(선·라벨·화살표)는 `--text-dim`으로 낮춘다(장식은 조용해야 한다).
- Status 강조 규칙: 현재 사용 가능한 항목(macOS Host "우선 대상")만 accent, 개발 중/예정은 `--text-dim` 텍스트 라벨.

### 2.4 컴포넌트 패턴 결정
| 패턴 | 적용 |
|---|---|
| 카드(surface+border) | **Features 4개만** |
| 틴트 밴드 + 리스트 행(카드 크롬 없음, 제목+설명) | Security 4개 |
| 정의형 행(좌: 항목명 / 우: 상태 라벨, 세로 리스트) | Status 5개 |
| 배지 | 히어로 1개 + 기술 스택 스트립에서만 |

---

## 3. 우선순위별 실행 지시 (파일별)

### P0 — 구조·위계 (가장 먼저)
1. **`src/index.css`**: §2.1~2.3의 토큰(타입·스페이싱·`--section-y`) 추가. `.section-container`/`.section-title`/`.section-lead`를 index.css로 이동해 단일 정의(파일별 중복·암묵 공유 제거). `html { lang: ko; }` 제거(C11).
2. **`src/components/FeatureGrid.tsx` + `FeatureGrid.css`**: sr-only h2 제거 → 가시 제목 "주요 기능" + 리드 1문장 추가(문구 제안: "설치된 PC의 화면을 승인된 Android 기기에서 여러 창으로 확인합니다." — 구현 시 확정). 카드 그리드는 유지하되 gap/카드 패딩 토큰 적용.
3. **`src/components/Security.tsx` + `Security.css`**: `Card` 사용 제거 → full-bleed surface 밴드 섹션(`section.security-band` > `.section-container`) 안에 2열 리스트 행으로 재구조화(C1). 리드 문장 유지. 제목 "보안"(C9).
4. **`src/components/Status.tsx` + `Status.css`**: `Card` 사용 제거 → 정의형 행 리스트(좌 항목명/우 상태 라벨, 행 사이 `--border` 구분선)로 재구조화(C7). 제목 "지원 현황"(C9). note는 하단 `--text-xs` 유지.

### P1 — 밀도·리듬·색상
5. **`src/components/Hero.tsx` + `Hero.css`** (C4): 배지 2개 → 1개 병합("MIT 라이선스 · 오픈 소스 · v0.1.4", 중립 스타일 — accent 틴트 제거 C6). CTA 2개는 유지.
6. **`src/content/site.ts`** (C4·C7): `hero.subcopy`를 2문장 이내로 축소(정보 보존: 다중 창·로컬 직접 연결·승인 기기 전송은 유지). `status.lead`에서 APK 파일명 제거(히어로 CTA와 중복) → "상태 기준일 2026-09-13 · 최신 릴리스 v0.1.4" 수준으로. `status.items`의 `highlight` 재검토 — macOS Host만 강조(C6).
7. **`src/components/Diagram.tsx` + `Diagram.css`** (C4·C6): accent → `--text-dim`, `margin: 4rem 0` → `margin-top: var(--space-6)`, padding `4rem 2rem` → `var(--space-6) var(--space-5)`, radius 16px → 12px. 시각 무게를 헤드라인보다 낮게.
8. **`src/components/Footer.tsx` + `Footer.css`** (C5): `margin-top: 4rem` 제거, `padding: var(--space-7) var(--space-4)`, 상단 보더 유지.
9. **`src/components/Card.tsx` + `Card.css`** (C5·C10): 패딩 24px→`--space-5`, title margin 12px→`--space-3`, title 1.2rem→`--text-sm`(600). `.card:hover` 제거.

### P2 — 마감
10. **`src/components/StackBadges.tsx` + `StackBadges.css`** (C8): `.section-title` 제거, `--text-xs` 소제목 "기술 스택" + 배지 열 + 설명 1문장의 컴팩트 스트립으로 축소. 섹션 패딩은 `--section-y` 절반 수준 허용.
11. **`src/content/site.ts`** (C8): `stack.description`의 "Rustra는" 오타 수정. ⚠️ 원 의도 단어 확인 필요(문맥상 "Tauri는" 또는 "브리지는"으로 추정) — 구현 전 사용자 확인.
12. **`src/components/Status.tsx`**: 상태 라벨에 `--text-sm` 적용, 행 높이 균일화.

### 변경 대상 파일 요약
`index.css`, `content/site.ts`, `Hero.tsx/.css`, `FeatureGrid.tsx/.css`, `Security.tsx/.css`, `Status.tsx/.css`, `StackBadges.tsx/.css`, `Card.tsx/.css`, `Diagram.tsx/.css`, `Footer.tsx/.css` / `App.tsx`는 구조 변경 없음.

---

## 4. 후속 구현 수용 기준 (acceptance)
- [ ] 페이지 전체 폰트 크기가 §2.1의 7단계 토큰 안에 있고 px 선언 없음.
- [ ] 모든 섹션의 세로 패딩이 `--section-y`로 균일, 섹션 간 중첩 margin 없음.
- [ ] 카드 크롬 요소는 Features 4개뿐. Security/Status는 행 기반.
- [ ] accent 사용처가 CTA·링크·상태 1곳의 3종 이하.
- [ ] 하이라이트 상태 라벨은 1개.
- [ ] 정보 손실 없음 — 모든 기존 사실(기능 4, 보안 4, 스택 5, 상태 5, 링크 4) 유지, 표현만 축소. 단 subcopy/APK 파일명 중복/Rustra 오타는 §3 지시대로 정리.
- [ ] `npm run build` 성공 + `npx -y react-doctor@latest . --verbose` 100/100 (AGENTS.md 품질 게이트).

## 5. 미해결 리스크
- "Rustra는" 오타의 원 의도 단어 미확정 → 구현 전 확인 필요.
- 섹션 제목 한국어 문구와 Features 리드 문장은 제안이므로 구현 시 카피 확정 필요.
- Security 밴드의 full-bleed 처리는 `.section-container` 밖 래퍼가 필요 → 반응형(모바일 좌우 패딩) 확인 필수.
