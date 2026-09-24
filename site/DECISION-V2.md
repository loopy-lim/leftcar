# Leftcar 랜딩 페이지 V2 아키텍처 결정 — Vite + React 전환 및 Cloudflare Pages 배포

- 상태: 확정 (attempt 0, decision)
- 상위 결정: `site/DECISION.md` (IA·카피·비주얼 — 카피와 디자인 토큰은 예외 없이 승계)
- 입력: audit-content 결과(정적 페이지 자산 감사), audit-repo 결과(저장소 통합 제약), intent(Vite + React 전환·Cloudflare Pages 배포)
- 산출물 대상: `site/`를 Vite + React(TS) 프로젝트로 전환, `dist` 정적 출력을 Cloudflare Pages에 배포

---

## 0. 카피·디자인 자산 승계 (변경 없음)

**audit-content의 감사 결과를 그대로 승계한다.** 이 문서는 전달 방식(빌드·구조·배포)만 결정하며, 콘텐츠와 비주얼은 1바이트도 바꾸지 않는다.

- 카피 전문: `DECISION.md` §3 (Hero 배지·헤드라인·서브카피·CTA 2개, Features 카드 4개, Security 리드+4항목, Tech Stack, Status 리드+5항목, Footer) 및 `site/index.html`에 구현된 최종 문구·링크 URL 전부.
- 디자인 토큰: 컬러 6개(`--bg #0b0e14`, `--surface #11151f`, `--border #1f2733`, `--text #e6ebf4`, `--text-dim #97a3b6`, `--accent #4cc2a8`), 시스템 폰트 스택 2종, 768px 단일 브레이크포인트, `clamp()` 타이포.
- 다이어그램: `figure[aria-label]` + 장식 요소 `aria-hidden` 시맨틱 구조와 시각 결과 그대로 유지. 애니메이션 0, 이미지 0.
- `prefers-reduced-motion: reduce` 대응과 카드 hover `border-color` transition 유지.
- audit-content의 "버릴 것"도 그대로 따른다: 단일 HTML 구조와 인라인 `<style>` 270줄은 Vite 템플릿 + React 컴포넌트 트리 + CSS 파일 분리로 대체하고, 전역 태그 셀렉터(`*`, `section`, `ul`, `footer`, `a`, `code`)는 리셋/전역 레이어와 컴포넌트 스코프 클래스로 분해한다.
- 외부 리소스 금지 원칙(이미지·CDN·웹폰트 0)은 Vite 전환 후에도 유지. 빌드 결과물에 외부 네트워크 요청이 발생하지 않아야 한다.

---

## 1. 결정 ① — `site/`는 워크스페이스 밖 독립 Vite + React (TS) 프로젝트로 유지

**결정: 유지 (루트 Bun 워크스페이스에 편입하지 않는다).**

근거 (audit-repo 결과 기반):

1. 루트 `tsconfig.json`에 `references` 필드가 없고 `include`가 `packages/*/src/**`, `apps/*/**`, `tools/**/*.ts`뿐이므로 site/는 이미 `tsc -b` 대상에서 완전히 배제되어 있다. 독립 `site/tsconfig.json`(target ES2022, moduleResolution bundler, jsx react-jsx, strict — 루트와 동일 기조)을 두면 루트 타입체인과 충돌하지 않고, `tsconfig.tsbuildinfo` 영향도 없다.
2. 루트 워크스페이스 글롭(`packages/*`, `apps/*`, `apps/*/modules/*`)은 site/와 매치되지 않는다. 편입하려면 루트 `package.json`·`bun.lock`을 건드려야 하고, 이는 저장소의 모든 워크스페이스 CI 런에 lockfile/캐시 파급을 일으킨다. site/는 공유 코드가 0이므로 워크스페이스 멤버십의 이득이 없다.
3. 의존성 방향: `site/ → (저장소 내부 어디도 참조 안 함)`, 저장소 모듈 → `site/` 참조도 금지. DECISION.md §1의 경계 원칙을 그대로 승계한다.
4. 버전 관례: `check:react-runtime`은 site를 검사하지 않지만, 저장소 관례에 맞춰 `react`·`react-dom`을 **정확히 19.2.3**으로 고정 선언한다(caret 금지).
5. 테스트 경계: 루트 vitest 글롭이 `site/**/*.test.*`를 흡수한다. 초기 구현에서는 site에 테스트 파일을 두지 않고, 렌더링 검증은 아래 §5의 스크립트로 수행한다. (향후 site 단위 테스트 도입 시 "루트 vitest에서 실행된다"는 점을 site README에 문서화.)

## 2. 결정 ② — 컴포넌트 경계와 데이터 흐름

### 2.1 컴포넌트 트리

```
App (src/App.tsx — 섹션 순서만 조립, 상태 없음)
├─ Hero          <header class="hero" id="hero">  배지 2 · h1 · 리드 문장 · CTA 2
│   └─ Diagram   <figure aria-label>  PC 프레임 → 네트워크 라벨·점선 화살표 → Android 프레임(창 3개)
├─ FeatureGrid   <section id="features">  Card × 4 (2×2 그리드)
├─ Security      <section id="security">  리드 문장 + Card × 4
├─ StackBadges   <section id="stack">  코드 배지 나열 + 보조 문장 1줄
├─ Status        <section id="status">  리드 문장 + status-list ul
└─ Footer        <footer id="footer">  링크 4개 + 소개 문장
```

- 컴포넌트명은 requirement 예시(Hero, FeatureGrid, Security, StackBadges, Diagram, Status, Footer)를 그대로 쓴다.
- FeatureGrid/Security/Status가 쓰는 카드 마크업이 동일하므로 공용 프리미티브 `Card`(h3 + children)를 하나만 둔다. 이 이상의 추상화는 만들지 않는다.
- 시맨틱 주의(audit-content 지적): Hero가 `<header>` 랜드마크이며 Diagram(`figure`)을 포함하는 현 구조를 **유지**한다. `<main>`은 features 이후 섹션만 감싸고, 전역에 header/main/footer 랜드마크가 각 1개씩 존재한다. h1(Hero) → h2(섹션) → h3(카드) 위계 유지.

### 2.2 데이터 흐름 — 카피는 constants 모듈로 분리

- 단일 방향 흐름: **constants → 컴포넌트 → 정적 JSX**. 상태·context·effect·이벤트 핸들러 0 (인터랙션은 CSS transition만, DECISION.md §4 승계).
- `src/content/site.ts` 하나에 모든 카피·링크·버전 상수를 모은다(타입 정의 동반: `Feature`, `SecurityItem`, `StatusItem`, `FooterLink` 등). 릴리스 갱신 시(v0.1.4 → v0.1.5) 수정 지점이 이 파일 하나로 한정된다 — DECISION.md §7 리스크("버전 수동 갱신")의 완화책.
- 컴포넌트는 필요한 constants를 직접 import한다. App에서 카피를 props로 굴리는 배관은 만들지 않는다(정적 페이지에 props 전달 계층은 비용만 추가).
- CSS: `src/index.css`(리셋 + `:root` 토큰 6개 + body/타이포 베이스)와 컴포넌트별 CSS(`Hero.css`, `Diagram.css` 등, 클래스 스코프)로 분리. CSS 프레임워크·CSS-in-JS 도입 금지 — 런타임/의존성 0 원칙.

### 2.3 파일 구조

```
site/
├─ index.html            # Vite 엔트리 (lang="ko", title·meta description은 기존 문구 재사용)
├─ package.json          # react/react-dom 19.2.3 고정, vite·@vitejs/plugin-react·typescript·wrangler
├─ tsconfig.json         # ES2022 / bundler / react-jsx / strict
├─ vite.config.ts
├─ wrangler.toml         # name = "leftcar-site", pages_build_output_dir = "dist"
└─ src/
   ├─ main.tsx
   ├─ App.tsx
   ├─ index.css
   ├─ content/site.ts
   ├─ components/{Hero,Diagram,FeatureGrid,Security,StackBadges,Status,Footer}.tsx (+css)
   └─ components/Card.tsx
```

## 3. 결정 ③ — Cloudflare Pages: 정적 `dist` + `wrangler pages deploy`, Functions 미사용

**결정: 빌드 산출물(`site/dist`)을 wrangler direct upload로 배포한다. Cloudflare Functions, Durable Objects, 서버 로직은 일절 사용하지 않는다.**

근거:

1. 산출물이 순수 정적(HTML/CSS/JS 자산)이므로 Pages의 역할은 CDN 호스팅이 전부다. Functions를 넣으면 실행 경로·비용·보안 검토 범위만 늘어난다.
2. 외부 리소스 0 원칙과 정합 — 배포 대상이 앱 자체 에셋뿐임이 배포 구조로 보장된다.

브랜치/프로덕션 전략:

| 트리거 | 동작 |
|---|---|
| `main` push | `wrangler pages deploy dist` → **프로덕션** 배포 (`<project>.pages.dev`, Pages 프로젝트의 production branch = main) |
| PR / 그 외 브랜치 | 배포하지 않음. 빌드 + typecheck + React Doctor 검증만 수행 |
| 수동 프리뷰 | 필요 시 `bunx wrangler pages deploy dist --branch=<브랜치명>`으로 프리뷰 URL 생성 (워크플로에 자동화하지 않음) |

설정 파일·시크릿:

- `site/wrangler.toml`에 `name = "leftcar-site"`, `pages_build_output_dir = "dist"`만 선언해 설정을 저장소 안에 둔다(대안: CLI 플래그로 매번 전달 — 설정 분산, 기각).
- 최초 1회 프로비저닝: `wrangler pages project create leftcar-site --production-branch=main` (사람이 수행, 문서화 대상).
- GitHub 시크릿: `CLOUDFLARE_API_TOKEN`(권한: Cloudflare Pages Edit), `CLOUDFLARE_ACCOUNT_ID`. wrangler는 site devDependency에 **정확한 버전으로 고정**하고 워크플로에서 `bunx wrangler pages deploy dist`로 실행한다 — 서드파티 액션 도입을 피하고 버전을 site lockfile로 고정한다.
- 커스텀 도메인은 범위 밖(향후 Pages 대시보드에서 수동 연결).

## 4. 결정 ④ — GitHub Actions 워크플로: 신설한다

**결정: `.github/workflows/deploy-site.yml`을 새로 만든다. 기존 `ci.yml`은 수정하지 않는다.**

- 기존 ci.yml의 rust-fast/ts 레인은 site와 무관하다(audit-repo: 루트 typecheck·build·contract 모두 site 미포함). site 배포를 ci.yml에 얹으면 관심사가 섞이고, rust 작업이 끝나야 ts가 도는 직렬 구조 때문에 배포가 불필요하게 늦어진다.
- 워크플로 설계:
  - 트리거: `push`(branches: main, paths: `site/**` + 워크플로 파일 자신), `pull_request`(동일 paths).
  - 잡: ① checkout → setup-bun(1.4.1) → `cd site && bun install --frozen-lockfile` → `tsc --noEmit`(site typecheck) → `vite build` → 저장소 루트에서 `npx -y react-doctor@latest . --verbose`(100/100 게이트) → ② main push인 경우에만 `wrangler pages deploy dist`.
  - `concurrency`: site 배포는 그룹 지정 + cancel-in-progress (중복 배포 방지).
  - 액션 핀: ci.yml 관례대로 SHA 핀 + 버전 주석.
- 저장소 컨벤션 검증(audit-repo 후속): site TS 파일 추가 후 `bun run test:architecture` 1회 실행해 architecture-check가 오탐하지 않음을 확인한다.

## 5. 검증 게이트 (구현 완료 조건)

1. `cd site && bun run build`(vite build) 성공 — 외부 네트워크 요청 없이 dist 생성.
2. **React Doctor 100/100**: 저장소 루트에서 `npx -y react-doctor@latest . --verbose`. 이슈는 ignore 규칙이 아니라 소스 수정으로 해소한다(AGENTS.md 게이트).
3. site typecheck: `cd site && tsc --noEmit` 성공.
4. 렌더링 검증: `vite preview`로 dist를 서빙하고 playwright-core(루트 devDependency 재사용, `bun run setup:browser`로 chromium 확보)로 확인 — 6개 섹션 id 존재, h1 텍스트, CTA 2개 href, 다이어그램 `aria-label`, 본문 배경 토큰 적용을 단언하는 스크립트를 site 내에 둔다.
5. `bun run test:architecture` 1회 통과 확인(§4).
6. 루트 `bun run typecheck`·`bun run test`가 site 파일 유무와 무관하게 기존과 동일하게 통과함을 확인(경계 비오염 증명).

## 6. 기각한 대안과 근거

| 기각한 대안 | 근거 |
|---|---|
| **site/를 루트 Bun 워크스페이스에 편입** | 공유 코드 0인데 루트 `package.json` 워크스페이스 글롭·`bun.lock`·CI 캐시 전체에 파급. 루트 vitest 글롭이 site 테스트를 흡수하고, `tsc -b`에 references를 추가해야 하는 등 루트 빌드 시스템 수정을 강요한다. audit-repo가 확인한 "site는 루트 대상에서 완전 배제" 상태가 가장 깨끗한 경계다. 독립 package.json + 독립 tsconfig로 동일한 격리를 더 적은 변경으로 얻는다. |
| **Next.js / Astro 등 메타 프레임워크 도입** | 단일 랜딩에 SSG/아일랜드 아키텍처·프레임워크 전용 어댑터(next-on-pages, astro adapter)는 과잉. 의존성 표면이 커지고 배포 산출물이 프레임워크 런타임을 동반해 "외부 리소스 0·최소 산출물" 원칙과 충돌한다. intent가 Vite + React를 지정했으므로 그 범위 안에서 가장 얇은 조합(Vite SPA, 라우터 0, 메타 프레임워크 0)을 채택. |
| **Cloudflare Workers(정적 자산 + Worker) / Functions 사용** | 서버 로직 요구가 없다. 실행 경로·권한·비용 검토 범위만 늘어나고 Pages 정적 호스팅으로 충분. |
| **Cloudflare Pages Git 연동(대시보드에서 빌드)** | 빌드 설정이 저장소 밖 대시보드에 존재하게 되어 리뷰 불가능. 이미 GitHub Actions가 저장소 표준 CI이므로 배포도 같은 워크플로 안에서 wrangler direct upload로 수행한다. |
| **기존 ci.yml에 배포 잡 추가** | rust 빌드 체인에 site 배포가 결합되어 배포 지연·관심사 혼재. 독립 워크플로 + path 필터가 site 변경 시 최소 비용 경로. |

## 7. 남은 리스크

- 릴리스 버전 갱신(v0.1.5 등) 시 `src/content/site.ts` 수동 수정 필요 — 수정 지점이 단일 모듈로 한정된 것까지가 완화책이며, 자동 동기화는 범위 밖.
- Cloudflare 계정·시크릿 프로비저닝(프로젝트 생성, 시크릿 등록)은 사람이 수행해야 하는 단계로, 구현 워커가 이를 대신할 수 없다. 미완료 상태에서는 워크플로 배포 잡이 실패한다(빌드·검증 잡은 정상).
- react-doctor를 저장소 루트에서 실행하면 site/가 별도 프로젝트임을 도구가 인식하지 못할 수 있다. 100/100 미달 시 site 소스 기준으로 수정하되, 도구가 site/를 스캔 자체를 하지 않는다면 그 사실을 결과 보고에 명시한다(게이트 통과로 위장하지 않음).
- wrangler/버전 고정 값과 Pages 프로젝트명(`leftcar-site`)은 구현 시 실제 계정 상황에 맞춰 확정하되, 본 문서의 전략(정적 배포·production branch = main)은 불변으로 한다.
