# 사이트 보안·하드닝 리뷰 (site/)

- **일자**: 2026-09-25
- **범위**: `site/**` (빌드 설정 포함: `vite.config.ts`, `wrangler.toml`, `index.html`, `src/**`, `package.json`, 배포 워크플로)
- **성격**: 진단 전용 — 구현 없음
- **검증 방법**: 소스 전수 확인 + `site/dist` 빌드 산물 대조 + `docs/07-security-privacy.md`·`README.md`와 사이트 보안 주장 교차 검증

---

## 1. 확인된 이슈

### [높음] H1 — Cloudflare Pages 보안 헤더 전무 (`_headers` 파일 부재)

- **근거**: `site/` 및 `site/dist/`에 `_headers`(또는 `_redirects`) 파일이 존재하지 않음 (`find site -maxdepth 1 -name "_headers"` 결과 없음). 배포가 순수 정적 서빙(`wrangler.toml: pages_build_output_dir = "dist"`)이라 서버 측 헤더 주입 경로도 없음.
- **결과**: CSP, `X-Content-Type-Options`, `Referrer-Policy`, `Permissions-Policy`, `frame-ancestors`/`X-Frame-Options`, HSTS 등이 하나도 설정되지 않은 상태로 서빙됨. 클릭재킹 프레임 허용, MIME 스니핑, 리퍼러 누출, 인라인 주입 시 무제약 스크립트 실행에 대한 방어선이 0개.
- **수정**: `site/public/_headers`(또는 `dist` 루트 복사)에 `/*` 대상으로 CSP + 위 헤더 전체를 선언한다.
- **권장 값(현 구조에 맞춤)**:
  ```
  /*
    Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
    X-Content-Type-Options: nosniff
    X-Frame-Options: DENY
    Referrer-Policy: strict-origin-when-cross-origin
    Permissions-Policy: camera=(), microphone=(), geolocation=(), usb=()
    Strict-Transport-Security: max-age=31536000; includeSubDomains
    Cross-Origin-Opener-Policy: same-origin
  ```
  - 이 CSP는 아래 §3-1의 사실(인라인 스크립트/스타일 0건, 외부 오리진 0건) 덕분에 `'unsafe-inline'`·`'unsafe-eval'` 없이 바로 적용 가능함.
  - HSTS `includeSubDomains`는 같은 Cloudflare 계정의 다른 서브도메인 사용 계획과 충돌 없는지 확인 후 적용.

### [중간] M1 — head 완결성: og:/twitter: 메타 전무, favicon 부재, canonical/robots 없음

- **근거**: `site/index.html:1-14` — `charset`·`viewport`·`title`·`description`만 존재. `og:title`, `og:description`, `og:image`, `og:url`, `twitter:card` 등 소셜 메타가 1개도 없음. `favicon` 링크가 없어 브라우저가 `/favicon.ico`를 추측 요청(404). `site/public/` 디렉터리 자체가 없음.
- **영향**: 링크 공유 시 프리뷰 이미지·제목이 없고, favicon 404는 매 방문마다 불필요한 요청. 보안 관점에서도 SNS/메신저 공유 시 사이트 식별 정보가 없어 유사 도메인 피싱과 정품 구분이 어려워짐.
- **수정**: `site/public/`을 만들어 favicon(ico/svg/png)과 `og:image`(1200×630)를 넣고, `index.html`에 og:/twitter:/canonical/`theme-color` 메타를 추가한다.

### [낮음] L1 — 릴리스 버전·URL 하드코딩 (갱신 누락 위험)

- **근거**: `site/src/content/site.ts:23` (`badge: 'MIT 라이선스 · 오픈 소스 · v0.1.4'`), `site/src/content/site.ts:27-28` (`cta1` 라벨·URL `v0.1.4`), `site/src/content/site.ts:96` (`status.lead: '... 최신 릴리스 v0.1.4'`).
- **영향**: 새 릴리스 시 3곳을 수동 갱신해야 하며 누락되면 "구버전 APK가 최신"으로 표시됨. 사용자가 패치되지 않은 구버전을 받는 간접 보안 위험. 버전 정보 노출 자체는 오픈 소스 프로젝트에서 적절함.
- **수정**: 버전 문자열을 상수 1곳으로 추출해 3곳이 참조하게 하고, 릴리스 시 갱신 대상을 CI나 체크리스트로 강제한다.

### [위생] W1 — 외부 링크에 `target="_blank"` 미사용 → `noopener` 이슈 없음

- **근거**: `site/src` 전체에서 `target=` 사용 0건(CTA 2개: `site/src/components/Hero.tsx:16-17`, 푸터 4개: `site/src/components/Footer.tsx:14-17`). 현재 창에서 그대로 이동하므로 reverse tabnabbing 위험 없음.
- **수정**: 유지 가능. 새 탭 열기를 원하면 반드시 `target="_blank"`와 `rel="noopener noreferrer"`를 함께 추가한다.

### [위생] W2 — 스택 설명에 내부 용어 "Rustra" 무설명 노출

- **근거**: `site/src/content/site.ts:90` — 배지 목록(`site.ts:89`)에는 없는 "Rustra"가 방문자에게 설명 없이 등장. 내부 도구명(`docs/04-rustra-control-contracts.md`)이라 외부 방문자에게는 오탈자처럼 보임(AI-티·정확성 문제, 보안 아님).
- **수정**: 방문자 관점 문구로 교체하거나(예: "타입 안전 Rust↔TS 제어 계약") 배지에 정식 명칭과 설명을 추가한다.

### [위생] W3 — 배포 워크플로 자동 트리거 비활성 상태

- **근거**: `.github/workflows/deploy-pages.yml:5-18` — `push`/`pull_request` 트리거가 주석 처리되고 `workflow_dispatch`만 활성. 시크릿은 표준 GitHub Secrets으로만 사용되어 유출 경로는 없음. 보안이 아니라 운영 리스크(사이트 최신성 보장 안 됨).
- **수정**: 시크릿 준비 완료 후 주석을 되돌려 main 푸시 자동 배포를 재활성화한다.

---

## 2. 보안 주장 vs 실제 문서 대조 (과장 여부)

사이트에 표시된 보안 주장은 `docs/07-security-privacy.md`, `README.md:65`와 대조한 결과 **과장 없이 일치**함:

| 사이트 주장 (site.ts) | 문서 근거 | 판정 |
|---|---|---|
| 기기별 화면 승인 (s1) | Host 명시적 승인 요구 (docs/07) | 일치 |
| 입력 기본 OFF, 세션별 허용 (s2) | docs/01-product-requirements.md:41 | 일치 |
| QR 핀 Ed25519 핸드셰이크 + ChaCha20-Poly1305 AEAD (s3) | docs/07:55-56, 87, 390-393; README:65 | 일치 |
| 로컬 네트워크 전용, 공개 인터넷 제외 (s4) | docs/07:87; README:65 | 일치 |
| Windows Host "물리 기기 검증 진행 중" (st2) | 상태 문서와 일치 — 축소/과장 없음 | 일치 |

- 다이어그램의 "로컬 네트워크 · 암호화" 라벨(`site/src/components/Diagram.tsx:19`)도 문서와 부합.
- 유일한 주의점: 사이트는 "암호화"를 단정하지만 문서상 Windows 파일 권한 ACL 의존 등 세부 한계(docs/07:92)는 다루지 않음. 랜딩 문서로는 허용 범위이나, 더 강한 보안 마케팅(예: "엔드투엔드", "제로 트러스트")을 추가하는 순간 과장이 되므로 현 문구 수준을 유지할 것.

---

## 3. 양호 항목 (CSP 도입을 쉽게 만드는 구조)

1. **인라인 스크립트/스타일 0건**: `src/**`에서 `dangerouslySetInnerHTML`·`style=` 인라인 0건, 빌드 산물(`dist/index.html`)도 외부 에셋 2개(`<script>`, `<link>`)만 참조 → `script-src 'self'; style-src 'self'`가 무충돌로 적용 가능.
2. **외부 폰트·CDN 의존 0건**: 모든 CSS가 자체 호스팅(`src/*.css`), 시스템 폰트 스택 사용 → `connect-src`/`font-src`를 `'self'`로 잠겨 있음. 서드파티 추적·공급망 노출 없음.
3. **소스맵 미생성**: `vite.config.ts`에 `build.sourcemap` 설정 없음 → `dist/assets/`에 `.map` 없음, 내부 경로 노출 없음.
4. **런타임 비밀정보 없음**: 사이트는 정적 콘텐츠만 렌더링, API 키·토큰·엔드포인트 없음.
5. **의존성 최소**: 런타임 의존성이 react/react-dom뿐 → 공급망 공격 면 최소.

---

## 4. 요약 — 상위 이슈 (심각도 순)

| # | 심각도 | 이슈 | 핵심 수정 |
|---|---|---|---|
| 1 | 높음 | 보안 헤더 전무 — `_headers` 부재 (CSP·XCTO·Referrer-Policy·Permissions-Policy·HSTS·frame-ancestors) | `site/public/_headers` 신설, §1 권장 값 적용 |
| 2 | 중간 | head 완결성 — og:/twitter: 메타 0건, favicon 부재(404), canonical/robots 없음 | `site/public/` 에셋 추가 + `index.html` 메타 보강 |
| 3 | 낮음 | 릴리스 버전 하드코딩 3곳 — 갱신 누락 시 구버전 APK 안내 | 버전 문자열 단일 상수화 + 갱신 절차 강제 |
| 4 | 위생 | 외부 링크 `target="_blank"` 미사용 — 현재는 안전, 신규 탭 추가 시 `rel="noopener noreferrer"` 필수 | 유지 또는 target+rel 세트 추가 |
| 5 | 위생 | 스택 설명의 미설명 내부 용어 "Rustra" | 방문자용 문구로 교체 |
| 6 | 위생 | 배포 자동 트리거 비활성(운영 리스크) | 시크릿 준비 후 워크플로 재활성화 |

**결론**: 침해로 이어질 수 있는 취약점(주입·비밀정보 노출·공급망)은 발견되지 않았음. 최우선 과제는 H1(`_headers` 한 파일로 해결되는 하드닝)이며, 나머지는 완결성·운영 위생 항목.
