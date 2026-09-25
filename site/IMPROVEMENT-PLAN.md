# Leftcar 랜딩 통합 개선 계획 (IMPROVEMENT-PLAN)

- 기준 워킹트리: DESIGN-AUDIT P0~P2 재구조화가 반영된 미커밋 상태 (2026-09-26 기준)
- 입력 문서: `site/DESIGN-AUDIT.md` (구조화 지시), `site/AUDIT-VERIFICATION.md` (수용 기준 검증), `site/SECURITY-REVIEW.md` (보안·하드닝), `site/AI-LOOK-REVIEW.md` (잔여 AI-티 진단)
- 표기: 난이도(소/중/대) — 소: 단일 파일 소규모 수정, 중: 다중 파일 또는 신규 에셋, 대: 전면 재검증 필요
- 각 항목의 "완료 판정"은 검증 가능한 명령·조건으로 기술한다.

## 0. 선행 리뷰 종합 판정

- **DESIGN-AUDIT §4 수용 기준**: 7항목 중 6 PASS, 1 부분 충족(⑥ "Rustra는" 오타 미이행). 빌드 성공 + react-doctor 100/100 게이트는 **루트·site 스코프 양쪽에서 실제 통과 확인**.
- **보안**: 침해성 취약점(주입·비밀정보·공급망) 0건. 그러나 **보안 헤더 전무(H1)** 로 하드닝 기본선이 비어 있음.
- **탈AI화**: 구조 혼잡은 해소됐으나 "정확하지만 주인 없는" 완성도 — 히어로 템플릿 골격, 기본 폰트 스택, 빈 상자 모식도, 획일적 카피가 잔존.

### 문서 간 상충 판정 (기록용)

| 상충 지점 | 관련 문서 | 판정 |
|---|---|---|
| Security s3 문장(알고리즘 나열)을 독자 언어로 번역할 것인가 | AI-LOOK B1(번역 제안) vs SECURITY §2(현 문구 수준 유지 권고) | **번역하되 사실 수준 유지.** 독자 언어로 풀어 써도 "암호화한다"는 단정 수준은 그대로 유지한다. "엔드투엔드/제로 트러스트" 같은 강한 마케팅 표현 추가는 금지(SECURITY 판정 존중). 알고리즘 명칭(Ed25519, ChaCha20-Poly1305)은 랜딩에서 빼고 docs에 유지 |
| 히어로 CTA 라벨의 버전 제거(A1) vs 버전 하드코딩 상수화(L1) | AI-LOOK A1 vs SECURITY L1 | **둘 다 수행.** CTA 라벨에서 버전을 제거해 라벨을 안정화하고, 남은 버전 노출 2곳(badge, status.lead)은 단일 상수로 추출해 갱신 누락 위험 제거. 상호 충돌 없음 |
| "Rustra는" 처리 | DESIGN-AUDIT §3-11(사용자 확인 후 수정) vs AI-LOOK B3(주어 제거 임시안) vs SECURITY W2(방문자용 문구 교체) | **근본 원인 동일 — 하나의 항목으로 통합.** "Rustra"는 내부 도구명이자 오타로 추정되는 미확정 단어. 원 의도 확정 전까지는 주어를 제거한 방문자용 문구로 교체(3안 모두 충족하는 최소 침범 해법). §4-⑥ 미이행 상태는 이 항목 완료 시 해소 |

---

## 1. 지금 당장 고칠 것 (수용 기준 미충족 + 보안 상위 이슈)

### 1-1. [보안·높음] Cloudflare Pages 보안 헤더 신설
- **대상**: `site/public/_headers` (신규), 빌드 산물 dist 복사 경로 확인
- **내용**: `/*` 대상 CSP(`default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`) + `X-Content-Type-Options: nosniff` + `X-Frame-Options: DENY` + `Referrer-Policy: strict-origin-when-cross-origin` + `Permissions-Policy` (카메라·마이크·위치·usb 차단) + HSTS + COOP. 인라인 스크립트/스타일 0건, 외부 오리진 0건이 확인됐으므로 `'unsafe-inline'`·`'unsafe-eval'` 없이 적용 가능.
- **주의**: HSTS `includeSubDomains`는 같은 Cloudflare 계정의 타 서브도메인 계획과 충돌 없는지 확인(SECURITY 리뷰 유의사항).
- **난이도**: 소
- **완료 판정**: (1) `site/public/_headers` 존재, (2) 배포(또는 `wrangler pages dev`) 후 응답에 상기 헤더 6종 이상 존재, (3) `bun run build` 성공, (4) react-doctor 100/100(site 스코프).

### 1-2. [미충족·즉시] "Rustra는" 오타/내부 용어 해소 — §4-⑥ 유일 미이행
- **대상**: `site/src/content/site.ts:90` (`stack.description`)
- **내용**: 주어를 제거한 방문자용 문구로 교체. 예: "명령·상태·오류의 경계는 Rust와 TypeScript 사이의 계약으로 관리합니다." 원 의도 단어("Tauri는" 추정)가 확정되면 주어 복원 가능.
- **난이도**: 소
- **완료 판정**: `site.ts`에서 "Rustra" 문자열 0건(`grep -r "Rustra" site/src` 빈 결과) + §4-⑥ 재검증 PASS (정보 손실 없음).

### 1-3. [보안·중간] head 완결성 — 소셜 메타·favicon·canonical
- **대상**: `site/index.html`, `site/public/` (신규 디렉터리)
- **내용**: favicon(ico/svg/png) + og:image(1200×630) 에셋 추가, `og:title/description/image/url`, `twitter:card`, `canonical`, `theme-color` 메타 추가. favicon 부재로 인한 `/favicon.ico` 404 요청 제거.
- **난이도**: 소~중 (에셋 제작 포함 시 중)
- **완료 판정**: `dist/index.html`에 og: 메타 4종 이상 + favicon 링크 존재, `site/public/favicon.*` 존재, favicon 404 미발생(로컬 서빙 확인).

### 1-4. [보안·낮음→위생] 릴리스 버전 문자열 단일 상수화
- **대상**: `site/src/content/site.ts:23` (badge), `:27-28` (cta1), `:96` (status.lead)
- **내용**: `SITE_VERSION = 'v0.1.4'` 상수 1곳 정의, 3곳이 참조. 릴리스 시 갱신 대상이 1곳이 되도록. CTA 라벨 자체에서 버전 제거는 2-1(A1)과 병행.
- **난이도**: 소
- **완료 판정**: `site.ts`에서 `v0.` 리터럴 출현 1곳(상수 정의) 이하 + `bun run build` 성공.

---

## 2. 디자인 개선 — 탈AI화 상위 항목 (카피 제안 포함)

### 2-1. [P1] 히어로 템플릿 골격 탈피 — 배지 강등 + CTA 정리
- **대상**: `site/src/components/Hero.tsx` (`:9-19`), `site/src/components/Hero.css` (`.hero-badges`, `.hero-ctas`), `site/src/content/site.ts` (badge, cta 라벨)
- **내용**: pill 배지("MIT 라이선스 · 오픈 소스 · v0.1.4")를 pill 크롬 없는 텍스트 한 줄로 강등(`--text-sm`, `--text-dim`, 쉼표 나열). CTA 행 좌측 정렬로 꺾어 완전 대칭 해소(헤드라인·subcopy는 중앙 유지). CTA 라벨에서 버전 제거: "v0.1.4 Viewer APK 받기" → "APK 받기".
- **난이도**: 중
- **완료 판정**: `Hero.tsx`에 pill 배지 클래스(badge) 잔존 0건 + `border-radius: 999px` 선언 0건 + CTA 라벨에 "v0.1.4" 미포함 + 정보 손실 없음(MIT·오픈소스·버전 텍스트는 페이지에 유지) + 빌드·react-doctor 통과.

### 2-2. [P1] 카피 리라이트 — 독자 언어 + 종결·길이 편차
- **대상**: `site/src/content/site.ts` (features 4개 `:35-47`, security 4개 `:76-86`, featuresLead, security lead, hero.subcopy)
- **내용** (AI-LOOK B1·B2 제안안, 사실 수준 유지가 전제 — 상충 판정 참조):
  - s3 → "첫 연결은 QR 코드로 서로 확인하고, 그 뒤의 화면·입력 데이터는 모두 암호화해 주고받습니다." (알고리즘 명칭은 docs로)
  - featuresLead → "PC 화면을 폰에서, 여러 개로." (체언 종결 1개로 리듬 부여)
  - security lead → "화면은 민감합니다. 승인한 기기에만 보여 주고, 입력은 기본으로 잠가 둡니다."
  - hero.subcopy → 정의형 첫 문장("~뷰어입니다") 제거, 상황 서술로 교체. 교체 시 정보 손실 대조 필수.
  - 판정 기준: 소리 내어 읽었을 때 대상 사용자가 고개를 끄덕일 수 없는 문장은 docs로 이동.
- **난이도**: 소 (단일 파일)
- **완료 판정**: (1) site.ts 전체에서 "합니다" 종결 비율 70% 미만(편차 확인), (2) 기능 4·보안 4 사실 보존(SECURITY §2 대조 표 재검증 PASS), (3) 과장 표현("엔드투엔드" 등) 0건, (4) 빌드 통과.

### 2-3. [P1] 다이어그램 교체 또는 삭제
- **대상**: `site/src/components/Diagram.tsx`, `site/src/components/Diagram.css`
- **내용**: 우선순위 — (a) 실제 앱 스크린샷(PC 캡처 + Android 프레임)으로 교체, (b) 확보 불가 시 **삭제**(히어로는 헤드라인+subcopy+CTA로 종결, Features 첫 카드가 시각화 대체), (c) 유지 시 빈 `.window-body`에 실제 어휘("디스플레이 1", "화면", "입력") 삽입. 빈 상자+점선+"→" 텍스트 화살표 구도는 AI 모식도의 정석.
- **난이도**: 중~대 (스크린샷 확보 여부 의존)
- **완료 판정**: (a) 선택 시 실제 캡처 이미지 1장 이상 포함 + `img-src` CSP와 정합, (b) 선택 시 Diagram 컴포넌트 잔존 0건 + 여백 리듬 재검증(`--section-y` 균일 유지), (c) 선택 시 빈 상자 요소 0건. 공통: 정보 라벨("로컬 네트워크 · 암호화" 상당)은 문서와 일치(SECURITY §2 기준) 유지.

### 2-4. [P2] 칩 배지 박스 제거 — 스택 인라인 나열
- **대상**: `site/src/components/StackBadges.tsx/.css` (`:22-31`), `site/src/content/site.ts` (stack.badges)
- **내용**: 스택 배지 5개의 surface 박스 제거 → 인라인 텍스트 나열 "Rust workspace, Tauri 2, Expo/React Native, 네이티브 캡처/디코더, CI" (`--text-sm`, `--text-dim`). 텍스트를 일단 박스에 넣는 반사 패턴 제거. 2-1과 배치 처리 권장.
- **난이도**: 소
- **완료 판정**: StackBadges에서 배지 박스 스타일(surface+border) 잔존 0건 + 스트립 높이 축소 + 정보 5개 전부 유지 + 빌드 통과.

### 2-5. [P2] 중앙정렬 완전 대칭 완화
- **대상**: `site/src/components/Security.css`, `site/src/components/Status.css` (`.status-note`), 히어로는 유지
- **내용**: 본문형 콘텐츠(Security 리스트, Status 행)의 섹션 제목·리드·노트를 좌측 정렬로. 히어로만 중앙 유지. 리스트·행은 이미 좌측 정렬이므로 제목만 바꿔도 시선 흐름 정렬.
- **난이도**: 소
- **완료 판정**: Security·Status 섹션의 `text-align: center` 0건(히어로·Footer 제외) + react-doctor 통과.

### 2-6. [P3] 아이덴티티 폰트 1개 도입 + 팔레트 근거 결정
- **대상**: `site/src/index.css:32` (font-family), `:2-8` (색상 토큰)
- **내용**: (a) 헤드라인 전용 디스플레이 서체 1개만 self-host 도입(본문은 시스템 스택 유지 — 비용 최소, 대비 최대). 후보 2개 이상 비교 후 확정. Pretendard는 이미 새 기본값이므로 회피. (b) 다크 네이비+청록 팔레트에 대한 "왜"의 답을 결정 기록으로 남기거나 accent를 프로젝트 고유 신호색(예: 주황/노랑 계열)으로 교체. 색상 전면 교체는 정보 위계 재검증이 필요해 후순위.
- **난이도**: 대
- **완료 판정**: (a) 웹폰트 1종 self-host + 라이선스 명기 + `font-src 'self'` CSP 정합 + 성능 예산 내(빌드 CSS/JS 크기 증가 기록), (b) 색상 결정 근거가 문서로 기록됨(기록만으로도 "무비판 기본값" 판정 해소).

### 2-7. [P3] 브랜드 목소리 — "Leftcar" 이름의 카피 반영
- **대상**: `site/src/content/site.ts` (Status note 또는 Footer 영역)
- **내용**: 제품 철학 한 줄(예: "왼손에 들어 둘 화면")을 푸터 근처에 배치해 "Leftcar"라는 이름이 카피에 살도록. 톤(담백한 툴 vs 친근한 안내) 확정 후 진행.
- **난이도**: 소
- **완료 판정**: 문구 확정 + 반영 위치 기록 + 정보 위계 훼손 없음(섹션 구조 변경 없음).

---

## 3. 위생·후속 (P2)

### 3-1. px 간격 2건 토큰 치환
- **대상**: `site/src/components/Diagram.css:91` (`padding: 10px`), `:94` (`gap: 8px`)
- **내용**: `--space-2`/`--space-1`로 치환. §2.2 "px 직접 사용 금지" 규칙 정합(§4 기준 밖이나 다음 손질 시 함께).
- **난이도**: 소
- **완료 판정**: `site/src/**/*.css`에서 px 단위 spacing 선언 0건(1px 보더 제외).

### 3-2. 한국어 카피 공식 확정 기록
- **대상**: `site/src/content/site.ts` 섹션 제목 3개 + Features 리드 + 이 문서의 카피 제안 전체
- **내용**: 현재 감사 문서 제안안이 코드에 그대로 반영된 상태. 리드 승인 시 "확정"으로 기록하고 제안 상태 표기를 제거해 "주인 있는 문구"로 만든다.
- **난이도**: 소
- **완료 판정**: 확정 사실이 이 문서 또는 커밋 메시지에 기록됨 + site.ts 주석의 제안 상태 표기 제거.

### 3-3. 배포 자동 트리거 재활성화
- **대상**: `.github/workflows/deploy-pages.yml:5-18`
- **내용**: 시크릿 준비 완료 후 주석된 `push` 트리거 복원(운영 리스크 — 사이트 최신성 미보장 상태 해소).
- **난이도**: 소 (시크릿 준비가 선행 조건 — 사용자 작업 필요)
- **완료 판정**: main 푸시 1회 후 Pages 배포 자동 실행 확인.

### 3-4. react-doctor 게이트 범위 문서화
- **대상**: `AGENTS.md` 또는 CI 설정
- **내용**: 루트 workspaces가 `site/`를 포함하지 않아 루트 react-doctor 실행만으로는 site를 검증하지 못함. "site 변경 시 `cd site && npx -y react-doctor@latest . --verbose` 실행"을 게이트 문서에 명시.
- **난이도**: 소
- **완료 판정**: 게이트 문서(또는 CI)에 site 스코프 실행 지시 존재.

### 3-5. 외부 링크 정책 유지 확인
- **대상**: `site/src/components/Hero.tsx:16-17`, `site/src/components/Footer.tsx:14-17`
- **내용**: 현 상태(새 탭 미사용, `target=` 0건)는 reverse tabnabbing 위험 없음 — **유지**. 향후 `target="_blank"` 도입 시 `rel="noopener noreferrer"` 세트 필수.
- **난이도**: 없음 (변경 없음, 기록 목적)
- **완료 판정**: 해당 없음 — 신규 링크 추가 시 리뷰 체크리스트 항목으로만 운용.

---

## 4. 미해결 질문 (사용자 확인 필요)

| # | 질문 | 관련 항목 | 임시 조치 |
|---|---|---|---|
| Q1 | `site.ts:90` "Rustra는"의 원 의도 단어는? (추정: "Tauri는" 또는 "브리지는") | 1-2 | 주어 제거 문구로 교체하면 미확정 상태로도 게시 가능 |
| Q2 | 실제 앱 스크린샷 확보 가능 여부 — 빌드 가능한 macOS Host + Android 기기가 있는가? | 2-3 | 확보 불가 시 다이어그램 삭제가 기본안 |
| Q3 | 카피 톤 선호 — 담백한 툴 문서체 vs 친근한 안내체? hero.subcopy 전면 교체 시 정보 손실 허용 범위? | 2-2, 2-7 | 교체안은 정보 보존 대조 표와 함께 별도 제출 |
| Q4 | 헤드라인 폰트 후보 선호 + 웹폰트 self-host 용량 예산 허용치? | 2-6 | 미확정 시 시스템 스택 유지(현 상태 허용) |
| Q5 | 다크 네이비+청록 팔레트를 유지할 것인가? 유지한다면 "왜"의 근거 문구 제공 가능? | 2-6 | 근거 제공이 어려우면 accent 색 교체 검토 |
| Q6 | HSTS `includeSubDomains` — 같은 Cloudflare 계정의 타 서브도메인 사용 계획이 있는가? | 1-1 | 불확실 시 `includeSubDomains` 제외한 HSTS 먼저 적용 |
| Q7 | GitHub Pages 배포 시크릿 준비 상태 — 자동 배포 재활성화 시점? | 3-3 | 수동 `workflow_dispatch` 유지 |

---

## 5. 실행 요약 — 상위 3개

1. **1-1 보안 헤더 신설** (`site/public/_headers`, 소) — 한 파일로 하드닝 기본선 완성. 보안 리뷰 최상위 이슈.
2. **1-2 "Rustra는" 해소** (`site.ts:90`, 소) — §4 수용 기준 ⑥ 유일 미이행 항목 해소 + AI-티 "부주의 신호" 제거. Q1 답변 전에도 주어 제거안으로 진행 가능.
3. **2-2 카피 리라이트** (`site.ts`, 소) — 단일 파일 수정으로 "평균값 문장" 문제를 해소하는 최고 임팩트/비용비 항목. 1-4(버전 상수화)와 동일 파일이므로 묶어 처리 권장.

이후 순서: 2-1(히어로 골격) → 1-3(head 완결성) → 2-3(다이어그램, Q2 답변 후) → 2-4~2-5 → 2-6~2-7(Q4·Q5 답변 후) → §3 위생 일괄.

## 6. 전체 완료 판정 (최종 게이트)

- `cd site && bun run build` 성공
- `cd site && npx -y react-doctor@latest . --verbose` 100/100 (루트 실행은 site 미스캔이므로 불충분 — AUDIT-VERIFICATION §1-⑦ 근거)
- `grep -r "Rustra" site/src` 빈 결과
- 배포 환경에서 보안 헤더 6종 이상 확인
- §4 수용 기준 7항목 재검증 전부 PASS
