# FONT-DECISION — 아이덴티티 폰트 선정 (design.md §5 P3 해소)

- 날짜: 2026-09-25
- 범위: (1) 사이트 헤드라인용 디스플레이 서체 1종 선정 + self-host 적용 방법, (2) apps/host-desktop 폰트 방침, (3) apps/viewer-expo 폰트 방침
- 근거 문서: `site/design.md` §2.1/§5(열린 결정), `site/IMPROVEMENT-PLAN.md` 2-6a, Q4
- 상태: **확정**. 이 문서가 확정 판정이며, 후속 구현 태스크는 §3의 지시를 그대로 실행한다. design.md §2.1 갱신은 이 문서를 근거로 구현 태스크가 수행한다.

---

## 1. 선정 기준

1. **한글 지원** — 랜딩은 한국어. 헤드라인+섹션 제목에 쓰이므로 한글 글리프 필수.
2. **라이선스** — SIL OFL 1.1 등 self-host·재배포(서브셋 포함) 가능.
3. **파일 크기** — woff2 기준. 한국어 폰트는 1웨이트 500KB~1.3MB로 무거우므로 서브셋 전략이 전제. 예산: 폰트 합계 **100KB 이하**(랜딩 1페이지 기준, `font-display: swap` 병행).
4. **톤** — design.md의 "담백한 툴" 정체성 + "왼손에 들어 둘 화면"이라는 Leftcar 이름. 과장된 디스플레이체보다는 절제된 그로테스크/네오그로테스크. Pretendard·Noto Sans KR처럼 "새 기본값"이 된 서체는 회피(IMPROVEMENT-PLAN 2-6a 명시).

## 2. 후보 비교 (4종, 전부 SIL OFL 1.1 — ② 기준 충족)

| 후보 | 한글 | 라이선스 | woff2 크기 (1웨이트) | 톤 평가 | 판정 |
|---|---|---|---|---|---|
| **Wanted Sans** | 11,172 자모 완성체 (실측) | OFL 1.1 (`wanteddev/wanted-sans` v1.0.3, 저장소 `OFL.txt`) | complete 667KB / **사이트 사용 문자 서브셋 실측 30.4KB** | 네오그로테스크. UI 최적화 목적으로 설계되어 큰 크기에서도 절제된 뼈대. "담백한 툴"과 가장 일치. 라틴이 특히 좋아 "Leftcar" 워드마크 품질 확보 | **✅ 선정** |
| Paperlogy | 완성체 | OFL 1.1 (`ayaan-fonts/Paperlogy`, LICENSE.txt 실확인) | complete TTF 1.3MB / woff2 dynamic-subset 슬라이스 제공 | 지오메트릭+라운드. 최근 한국 스타트업 랜딩의 사실상 기본값으로 급속히 확산 — Pretendard와 같은 "흔한 기본값" 범주로 떨어질 위험이 가장 큼. 톤도 '툴'보다 '친근한 서비스' | 기각 |
| IBM Plex Sans KR | 완성체 | OFL 1.1 | korean 서브셋 497KB (fontsource 실측) | 엔지니어링 정체성으로 톤은 유효. 그러나 한국어 웨이트당 497KB로 후보 중 최대 — 커스텀 서브셋을 해도 원본 뼈대가 무겁고, 한글 글리프 완성도가 라틴 대비 떨어짐. IBM 브랜드 차용 인상도 부수 비용 | 기각 |
| Gmarket Sans | 완성체 | OFL 1.1 | 공식 woff2 미제공 (2차 가공본 의존) | 헤드라인 개성은 강력하나 이커머스 브랜드 연상이 강하고 톤이 크게 요란해 "담백한 툴"과 상충. 웨이트 3종뿐이고 공식 웹폰트 파이프라인 부재로 self-host 유지보수 리스크 | 기각 |

**측정 방법(재현 가능)**: `site/src/content/site.ts` + `site/index.html`의 사용 문자 293종(한글 211자 포함)을 추출해 `pyftsubset`으로 Wanted Sans Bold(700)를 서브셋 → **30.4KB woff2**. 카피가 늘어도 한글 사용 문자 수는 수십 자 단위로 증가하므로 100KB 예산 내 여유가 크다.

## 3. 최종 결정

### 3.1 사이트 — Wanted Sans Bold(700) 1웨이트, 커스텀 서브셋 self-host

- **다운로드 소스**: https://github.com/wanteddev/wanted-sans/releases/download/v1.0.3/WantedSans-1.0.3.zip (압축 내 `webfonts/static/complete/woff2/WantedSans-Bold.woff2`, 실측 667KB)
- **라이선스**: SIL Open Font License 1.1 — 저장소 `OFL.txt` 동봉분을 `site/public/fonts/OFL.txt`로 함께 두고, `@font-face` 주석에 출처 명기.
- **서브셋 구성**:
  - 웨이트: **700 한 개만** (design.md §2.1에서 히어로·섹션 제목 모두 700 — 추가 웨이트 불필요).
  - 문자집합: 사이트 카피 사용 문자 전체(현재 293종) + 여유 버퍼(자주 쓸 한글 기본 자모·숫자·문장부호). 목표 **50KB 이하**.
  - 서브셋은 **저장소 스크립트로 재생성**: `site/scripts/subset-font.sh`(pyftsubset, `--text-file`에 사용 문자 파일). 카피 대폭 변경 시 재실행하는 절차를 문서화.
- **적용**:
  - 파일: `site/public/fonts/WantedSans-Bold.subset.woff2`
  - `@font-face { font-family: "Wanted Sans"; font-weight: 700; font-display: swap; src: url("/fonts/WantedSans-Bold.subset.woff2") format("woff2"); }`
  - 새 토큰 `--font-display: "Wanted Sans", <기존 시스템 스택>` 정의 후 **히어로 헤드라인과 `.section-title`에만 적용**. 본문·리드·카드 등 나머지는 기존 시스템 스택 유지(비용 최소 원칙).
  - `<link rel="preload" as="font" ... crossorigin>` 1건. CSP `font-src 'self'`와 정합.
  - 코드 계열(`code, kbd, samp, pre`)은 ui-monospace 유지.
- **성능 예산 기록 의무**(IMPROVEMENT-PLAN 2-6a): 빌드 후 폰트 파일 크기와 CSS 증가분을 design.md 갱신 시 함께 기록.

### 3.2 host-desktop — 시스템 스택 유지 (폰트 번들링 안 함)

- **현 상태**: `apps/host-desktop/src/index.css:59-60`에 `--font-sans`/`--font-mono` 토큰 존재. `--font-sans`는 macOS/Windows 시스템 스택 + `"Inter"`(번들 없이는 로드되지 않는 이름).
- **결정**: **시스템 스택 유지.** 이유: ① 데스크톱 앱은 OS 네이티브 폰트가 도구의 기본 감각과 일치(SF Pro/맑은 고딕), ② 웹뷰 번들링은 앱 크기·라이선스 고지 비용 대비 위계 개선 이득이 없음, ③ 사이트와 달리 아이덴티티 표면(랜딩)이 아니라 작업 표면.
- **부수 권고**(desktop 디자인 태스크 참조): `--font-sans`에서 `"Inter"` 제거(로드 수단이 없는 죽은 토큰), `"SF Pro Text"`, `"JetBrains Mono"` 등 실재하지 않는 이름도 정리해 실제 폴백 체인만 남길 것.

### 3.3 viewer-expo — 시스템 기본 유지, 커스텀 폰트 번들 없음

- **현 상태**: `app/**`에서 `fontFamily`는 `"monospace"`(페어링 코드·시리얼 등)만 지정, 나머지는 `fontWeight`만으로 Android 기본 폰트(Roboto, 한글은 시스템 Noto Sans KR 폴백) 사용. `global.css`에 폰트 선언 없음.
- **결정**: **폰트 변경 없이 시스템 기본 유지.** 텍스트 위계 개선은 폰트 교체가 아니라 크기/웨이트/행간 토큰화로 해결한다. 이유: ① 커스텀 폰트는 `expo-font` + 네이티브 링크가 필요해 APK 크기와 네이티브 빌드 비용이 들고(이번 라운드 viewer-android 네이티브는 범위 밖), ② 한글 렌더링은 어차피 시스템 폴백이 담당하므로 번들의 실익이 라틴뿐, ③ 아이덴티티 표면은 사이트이고 뷰어 앱은 기기 화면 표현이 본업.
- **허용 예외**: `fontFamily: "monospace"` 사용처(페어링 코드)는 현재 유지. 가독성 문제가 실측으로 확인되면 그때 1웨이트만 번들 검토.

## 4. 구현 태스크 실행 순서 (요약)

1. `site/scripts/subset-font.sh` 작성(pyftsubset) → Wanted Sans Bold 서브셋 생성 → `site/public/fonts/` 배치(+OFL.txt).
2. `site/src/index.css`: `--font-display` 토큰 + `@font-face` 추가, 히어로/`.section-title`에만 적용.
3. design.md §2.1 갱신(글꼴 항목을 "시스템 스택 유지" → "디스플레이 Wanted Sans 700 + 본문 시스템 스택"으로) 및 §5 열린 결정에서 아이덴티티 폰트 항목 제거(폰트만; 팔레트는 별도).
4. 게이트: `cd site && bun run build` + `npx -y react-doctor@latest . --verbose` 100/100 + 폰트 파일 크기 기록.

## 5. 미해결 리스크

- Wanted Sans v1.0.3 저장소가 향후 구조 변경 시 재다운로드 경로 변화 — 서브셋 스크립트에 소스 URL·버전을 상수로 기록해 대응.
- 커스텀 서브셋은 카피에 없는 문자(예: 새 한글 조합)를 쓰면 폴백 폰트로 렌더링되어 섞여 보일 수 있음 — 스크립트에 버퍼 문자집합 포함, 카피 대폭 변경 시 재생성 절차를 design.md에 명기.
- 팔레트 근거(IMPROVEMENT-PLAN 2-6b)는 이 문서 범위 밖 — 별도 결정 필요.
