# Leftcar 랜딩 페이지 구조·카피·비주얼 결정 기록

- 상태: 확정 (attempt 0, decision)
- 기준 사실: `README.md` (상태 기준일 2026-09-13) + facts 워커 결과
- 산출물 대상: `site/index.html` (빌드 스텝 없는 단일 HTML/CSS 페이지)

---

## 1. 아키텍처 경계 결정

- `site/`는 저장소 빌드 시스템(Bun workspace, Cargo, Gradle)과 **완전히 독립**이다. 어떤 package.json·Cargo.toml 참조도 없고, 저장소 루트의 경계를 오염시키지 않는다.
- 산출물은 **파일 1개(`site/index.html`)** 로, CSS를 `<style>`로 임베드한다. 외부 이미지·CDN·웹폰트·JS 프레임워크 0. 어떤 정적 호스팅(GitHub Pages 등)에도 파일 복사만으로 배포된다.
- 의존성 방향: `site/index.html` → (없음). 저장소의 다른 모듈이 `site/`를 참조하는 일도 없어야 한다.

## 2. 정보구조(IA) 확정

```
1. Hero        — 한 문장 정의 + 서브카피 + CTA 2개(릴리스 APK / GitHub 저장소)
2. Features    — 기능 카드 4개 (2×2 그리드, 모바일 1열)
3. Security    — 보안 모델 4항목 (승인 기반·입력 기본 OFF 강조)
4. Tech Stack  — 스택 4요소 + 네이티브 데이터 경로 한 줄
5. Status      — 현재 상태/로드맵 요약 (기준일 명시)
6. Footer      — 저장소·문서·라이선스 링크
```

순서 근거: 방문자는 "무엇인가 → 무슨 기능이 있나 → 안전한가 → 뭘로 만들었나 → 지금 쓸 수 있나" 순으로 읽는다. 상태를 마지막에 두어 기대치를 조정하고, 보안을 기능 바로 다음에 둔다(화면 원격 전송 제품의 최대 구매 장벽이기 때문).

## 3. 섹션별 한국어 카피 확정

사실 원칙: facts의 **'사용 가능' 항목만 서술형으로 사용**, 검증 대기 항목은 전부 **'개발 중 / 검증 진행 중'** 표기. README의 "공개 APK와 개발 소스의 검사 결과를 섞지 않는다" 원칙을 그대로 따른다.

### Hero
- 배지: `MIT 라이선스 · 오픈 소스` + `v0.1.4 (2026-09-13)`
- 헤드라인(한 문장 정의 기반): **"승인한 PC 화면을, 손안의 여러 창으로"**
- 서브카피: "Leftcar는 Mac 또는 Windows PC의 화면을 Android 휴대폰과 태블릿에서 빠르게 보고, 필요할 때 키보드와 포인터로 조작하는 다중 화면 뷰어입니다. 사용자가 승인한 디스플레이만 신뢰하는 로컬 네트워크로 전송됩니다."
- CTA ①: **"v0.1.4 Viewer APK 받기"** → `https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4`
  - v0.1.4에는 `Leftcar-Viewer-0.1.4.apk`만 있음이 확인됨. macOS·Windows 설치판이 있다고 가정하는 CTA는 만들지 않는다.
- CTA ②: **"GitHub에서 보기"** → `https://github.com/loopy-lim/leftcar`

### Features (카드 4개)
1. **여러 화면, 여러 창** — "컴퓨터마다 Leftcar를 실행하고, 원하는 디스플레이를 Android에서 독립된 창으로 여세요. 특정 기기 전용 기능이 아니라 Android 표준 다중 창을 사용합니다."
2. **릴레이 없는 직접 연결** — "같은 로컬 네트워크 안에서 PC와 직접 연결됩니다. 압축 영상과 고주파 입력은 JavaScript를 거치지 않는 별도 네이티브 데이터 경로로 전송됩니다."
3. **키보드·포인터 조작** — "세션별로 입력을 허용하면 Android 화면에서 PC를 조작할 수 있습니다. 포인터 전송률은 영상 FPS의 2배로 제한됩니다."
4. **파일 전송·클립보드 동기화** — "선택 기능으로 파일을 주고받고 클립보드를 동기화합니다. 두 기능 모두 Host와 Viewer 양쪽의 동의 게이트를 유지합니다." (근거: docs/01-product-requirements.md §4.4)

### Security
- 리드 문장: "화면은 민감한 데이터입니다. Leftcar는 승인된 것만 전송하고, 입력은 기본적으로 꺼져 있습니다."
- 4항목:
  1. **기기별 화면 승인** — Host 사용자가 각 기기의 화면 접근을 명시적으로 승인해야 합니다.
  2. **입력 기본 OFF** — 새 세션은 원격 입력이 꺼진 상태로 시작하며, 세션별로 별도 허용합니다.
  3. **암호화된 제어·미디어** — QR로 핀된 호스트 Ed25519 키 핸드셰이크 뒤 ChaCha20-Poly1305 AEAD로 봉인합니다.
  4. **로컬 네트워크 전용** — 릴레이 없는 직접 연결만 전제하며, 공개 인터넷 노출은 범위에 넣지 않습니다.

### Tech Stack
- "Rust workspace + Tauri 2 (macOS/Windows Host) · Expo/React Native (Android Viewer) · 네이티브 캡처/디코더 · CI"
- 보조 문장: "Rustra는 Rust와 TypeScript 사이의 명령·상태·오류 계약에만 사용하고, 제품 로직은 TypeScript와 Rust로 작성합니다."

### Status
- 리드: "상태 기준일 2026-09-13. 최신 공개 릴리스는 v0.1.4이며 `Leftcar-Viewer-0.1.4.apk`를 제공합니다."
- 목록:
  - macOS Host — 우선 대상
  - Windows Host — 코드·교차 컴파일 완료, **물리 기기 검증 진행 중**
  - Linux Host — **선택적 후속 과제 (개발 예정)**
  - 장시간(10·30분) 영상 수용 검증 — **개발 중**
  - 앱 창 단독 캡처 — **후속 목표 (개발 예정)**
- 유의 문장: "공개 APK와 개발 소스의 검증 결과는 구분해 안내합니다."

### Footer
- `GitHub 저장소` / `릴리스` / `문서(docs/README.md)` / `MIT License`
- 저작권·소개 문장: "Leftcar — 로컬 네트워크 기반 PC→Android 다중 화면 뷰어"

## 4. 비주얼 방향 확정

- **톤**: 다크 톤 개발자 도구 랜딩. 거의 검정 배경 + 낮은 채도 표면 + 시안(teal) 액센트 하나. 액센트는 CTA·링크·다이어그램 강조에만 사용해 시각 소음을 최소화.
- **컬러 토큰(CSS 변수)**: `--bg: #0b0e14`, `--surface: #11151f`, `--border: #1f2733`, `--text: #e6ebf4`, `--text-dim: #97a3b6`, `--accent: #4cc2a8` (파생 색은 `color-mix` 또는 투명도로 파생, 새 변수 남발 금지).
- **폰트**: 시스템 스택만 사용. 본문 `-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Apple SD Gothic Neo", "Malgun Gothic", "Noto Sans KR", sans-serif` / 코드·기술 명칭 `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`. 웹폰트 로드 없음.
- **반응형**: 브레이크포인트 **768px 단 1개**. 이상 = 2열 그리드 + 가로 다이어그램, 이하 = 1열 + 세로 다이어그램. 유동 크기는 `clamp()`로 보조.
- **다이어그램**: 스크린샷 대신 **CSS만으로 그린 추상 모식도** — 왼쪽 PC 모니터 프레임(디스플레이 2개 겹침) → 중앙 전송선(애니메이션 없는 점선 + `→` 및 "로컬 네트워크 · 암호화" 라벨) → 오른쪽 Android 폰·태블릿 프레임 안에 독립된 창 3개(헤더 바 + 본문 블록). `div` + `border` + `gradient` + `::before/::after`로 구성.
- **인터랙션**: JS 없음. 링크/카드 hover는 CSS `transition`으로만.

## 5. 기각한 대안과 근거

| 기각한 대안 | 근거 |
|---|---|
| React/Next.js 등 프레임워크 앱 추가 | 빌드 스텝·런타임 의존성·배포 파이프라인이 필요해 단일 소개 페이지엔 과잉. 이 저장소는 이미 Bun/Cargo/Gradle 3중 툴체인 모노레포라 프런트엔드 빌드 추가는 경계 오염. 정적 파일 1개가 유지보수·배포 비용 최소. |
| 스크린샷/실제 UI 이미지 사용 | 공개 릴리스는 v0.1.4 APK뿐이고 Host 설치판 범위는 불명확. 실기기 검증 일부가 대기 중이라 특정 빌드의 스크린샷은 곧 부정확해지고, README의 "검증 결과를 섞지 않는다" 원칙과 충돌. CSS 다이어그램은 개념을 정확히 전달하고 코드로 유지보수된다. |
| 웹폰트(Pretendard 등) CDN 로드 | 외부 네트워크 의존·FOUT 발생. 한국어 시스템 폰트(Apple SD Gothic Neo/Malgun Gothic/Noto Sans KR)로 충분하고, '외부 리소스 0' 원칙과 일치. |
| JS 기반 애니메이션·인터랙션 | 정적 소개에 필요 없음. 보안·프라이버시를 파는 제품의 랜딩이 외부 스크립트를 1줄도 실행하지 않는 것 자체가 메시지. |
| 미검증 기능을 완료 문구로 표기 | facts의 검증 대기 항목과 README 원칙 위반. 모든 미검증 항목은 '개발 중'으로 통일. |

## 6. 다음 구현 워커 지침 요약

1. `site/index.html` 1개 파일로 위 IA·카피·토큰을 그대로 구현. 섹션마다 `<section id>` 부여(hero/features/security/stack/status).
2. `<html lang="ko">`, `<title>Leftcar — PC 화면을 Android 여러 창으로</title>`, meta description은 Hero 서브카피 재사용.
3. 다이어그램 마크업은 시맨틱하게 `figure` + `aria-label="PC에서 Android로 여러 독립 창을 전송하는 모식도"`로 감싸고 장식 요소는 `aria-hidden`.
4. 색 대비 확인: 본문 `#e6ebf4` on `#0b0e14`, 보조 `#97a3b6`는 본문보다 작은 크기에서만 사용.

## 7. 남은 리스크

- 릴리스 태그가 갱신되면(예: v0.1.5) CTA 링크·배지 버전 수동 갱신 필요. 정적 페이지는 README와 버전이 자동 동기화되지 않는다 → 푸터에 "기준일" 명시로 완화.
- 시스템 폰트 스택은 OS별 렌더링 차이가 있으나, 소개 페이지 품질에는 허용 범위로 판단.
