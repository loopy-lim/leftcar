# DESIGN-REVIEW — apps/viewer-expo UI 디자인 감사

- 날짜: 2026-09-25
- 기준 문서: `site/design.md`(탈AI 원칙 §1, 토큰 §2, 패턴 §3, 카피 §4), `site/FONT-DECISION.md` §3.3(viewer 폰트 방침)
- 범위: `apps/viewer-expo/app/index.tsx`, `app/**`, `src/**`(시각·위계 해당부), `global.css`, `app.config.ts`, 공유 토큰 `packages/ui-tokens/src/tokens.ts`(근거 인용)
- 성격: **감사 전용. 구현 없음.** 아래 수정안은 후속 구현 태스크의 입력이며, RN 런타임·네이티브 모듈(rustra-bridge, StreamLauncher 등) 동작은 일절 건드리지 않는 시각·위계 개선만 제안한다.
- 참고: 이 감사는 후속 run이며, 선행 run에서 site 폰트 적용(Wanted Sans Bold 서브셋 self-host)과 desktop 감사(`apps/host-desktop/DESIGN-REVIEW.md`)는 이미 완료되어 검증 대기 중이다. 이 문서는 viewer-expo 분만 다룬다.

---

## 0. 폰트 방침 (FONT-DECISION.md §3.3 → 구현 태스크 전달 사항)

**viewer-expo는 커스텀 폰트를 번들하지 않고 시스템 기본을 유지한다(확정).** 이번 감사에서도 `app/**`·`src/**`에서 `fontFamily` 선언은 0건이며, `fontFamily: "monospace"` 20건(페어링 코드·OTP·시리얼·엔드포인트)만 존재 — 이는 FONT-DECISION §3.3의 **허용 예외**와 일치하므로 유지한다.

구현 태스크에 전달하는 지시:

1. 폰트 교체·`expo-font` 번들링을 하지 않는다. 위계 개선은 **크기/웨이트/행간 토큰화**로만 해결한다.
2. 현재 사용 중인 `@leftcar/ui-tokens`의 `typography` 토큰(`packages/ui-tokens/src/tokens.ts:96-129`, xs 12 / sm 13 / base 15 / md 16 / lg 18 / xl 22 / xxl 26 + lineHeight 세트)이 `theme.ts:11`에서 re-export만 되고 **스타일 코드에는 단 1곳도 적용되어 있지 않다**(grep 실측: `typography` 참조 0건). 이것이 이번 라운드의 핵심 구현 과제다.
3. Android 기본(Roboto + 한글 Noto Sans KR 폴백)은 그대로 두며, 웨이트는 400/500/600/700 단계만 쓴다.

---

## 1. 축별 총평

| 축 | 판정 | 요지 |
|---|---|---|
| ① 화면 위계 (첫 화면에 핵심 노출) | **부분 충족** | 연결 상태에서는 핵심("화면 열기" CTA + 카탈로그 진입)이 명확. 미연결+이력 없을 때는 섹션 제목(12px)이 본문(14px)보다 작은 위계 역전과 G2형 균일 카드가 핵심을 흐림 |
| ② AI-티 패턴 (design.md §1) | **경미한 위반** | G2 균일 칩 그리드(FeatureCardsGrid), G1 경계성 pill 배지 2종. G3/G5/G6은 해당 없음 |
| ③ 게이트 UI 명확성 (페어링·승인·입력) | **개선 여지** | OTP 게이트 자체는 우수(20px monospace). 단 카메라 권한 오류의 재시도 버튼이 무스타일, hostDot이 중립색을 상태 점처럼 사용해 신호 예약 원칙 훼손 |
| ④ 다크 배경 대비·텍스트 크기 위계 | **미달 (최우선)** | dark `textMuted` #71717A / `textDim` #52525B가 12px 본문·제목에 다용도 — WCAG AA 미달(약 3.8:1 / 2.4:1). `fontSize: 11` 잔존 |
| ⑤ Android 다중 창·회전 견고성 | **대체로 견고** | `panel-density` 스케일링·SafeAreaView·useWindowDimensions 잘 갖춰짐. 가로 폰에서 콘텐츠 최대폭 캡 부재만 개선점 |

폰트는 시스템 기본(방침 확정)이므로 ④의 문제는 글꼴이 아니라 **토큰 부재 + 저대비 토큰 값**에서 온다.

---

## 2. 이슈 목록 (우선순위 순)

### P1. 다크 테마 저대비 텍스트가 12px 미세 크기와 결합 — 가독성 미달

- **근거**:
  - `packages/ui-tokens/src/tokens.ts:53-54` — dark `textMuted: "#71717A"`, `textDim: "#52525B"`
  - `packages/ui-tokens/src/tokens.ts:57` — dark `bgSurface: "#141417"` (실측 대비: `textMuted` ≈ **3.8:1**, `textDim` ≈ **2.4:1** — 12px 일반 텍스트 WCAG AA(4.5:1) 미달. `textSecondary` #A1A1AA는 ≈7.2:1로 충분)
  - `apps/viewer-expo/src/components/hub-styles.ts:227-233` — `sectionTitle`: fontSize 12 + `textMuted` + uppercase(설정 가이드 제목이 본문 `stepName` 14px보다 작은 위계 역전)
  - `apps/viewer-expo/src/components/hub-styles.ts:132-135` — `badgeSuccessText`: **fontSize 11** (토큰 문서 스스로 "10px/11px micro-fonts 배제"를 표방 — `tokens.ts:94` — 와 모순)
  - `apps/viewer-expo/src/catalog-styles.ts` — fontSize 38건 중 다수가 12px(51, 226, 235, 251, 299, 313, 356, 375, 410, 429…)
- **영향**: 뷰어 앱의 주 사용 환경(어두운 곳에서 폰으로 PC 화면 감시)에서 상태 라벨·섹션 제목·보조 정보가 읽히지 않는다. ④축 최대 리스크.
- **수정안**:
  1. dark 팔레트에서 `textMuted`를 `#8E8E96` 수준으로 상향해 `bgSurface` 대비 4.5:1 이상 확보(토큰 1곳 수정으로 앱 전파 — 단, `@leftcar/ui-tokens`는 desktop과 공유이므로 desktop 감사 문서와 상충 없는지 확인 후 변경). `textDim`은 텍스트 용도에서 제외하고 도트·아이콘 등 비텍스트로 용도 제한.
  2. `fontSize: 11` 전부 제거 → `typography.xs`(12) 이상.
  3. `sectionTitle`은 12px muted uppercase 대신 `typography.sm`(13) + `textSecondary` 600으로 상향하거나, 본문(14px) 위에 오는 제목이므로 15px로 올려 위계 복원.
- **난이도**: 소(토큰 값 상향) ~ 중(12px 전면 재조정) / **임팩트: 대**

### P2. 타이포 토큰 미적용 — fontSize/fontWeight 하드코딩과 700 남용

- **근거**:
  - `apps/viewer-expo/app/index.tsx:26-28` — `useAppTheme`만 사용, `typography` 미 import
  - `apps/viewer-expo/src/components/hub-styles.ts` 전체 — fontSize 11/12/13/14/15/17 하드코딩, fontWeight "700" 12건(topBarTitle, heroTitle, badgeSuccessText, primaryActionText, stepNum, sectionTitle, featureValue…)
  - `theme.ts:11`에서 `typography`를 export만 하고 소비처 없음(실측: `app/**`, `src/**`에서 `typography` 참조 0건)
- **영향**: FONT-DECISION §3.3이 지정한 "크기/웨이트/행간 토큰화"가 없는 상태라 위계가 700 웨이트 남용으로만 성립한다. 제목·상태·버튼이 전부 700이면 위계 신호로 기능하지 않고, 이는 design.md G4가 지적하는 "평균값 목소리"의 타이포판이다.
- **수정안**: `createHubStyles`/`catalog-styles`/각 화면 `createStyles`의 fontSize·lineHeight를 `typography.*` 토큰으로 치환한다. 웨이트는 제목 600, 본문 400, 강조 1단계(500)로 낮춰 700은 primary CTA 텍스트만 남긴다. `panel-density`(`applyPanelDensity`, `panel-density.ts:57-92`)는 fontSize를 배율 적용하므로 토큰 치환과 정합 — 충돌 없음.
- **난이도**: 중 / **임팩트: 중~대** (FONT-DECISION viewer 방침의 본령)

### P3. 미연결 첫 화면의 위계 — G2 균일 카드 그리드 + 제목 역전

- **근거**:
  - `apps/viewer-expo/src/components/FeatureCardsGrid.tsx:17-39` — 표면·형태가 같은 카드 2개를 2열 반복(hub-styles.ts:271-290 `featureGrid`/`featureCard`), 서로 다른 질문(성능 "초저지연 60 FPS" / 멀티창 "멀티 디스플레이")을 같은 그릇에 담음 — design.md **G2 위반**. 카드 안 `featureValue`는 값이 아니라 제목 텍스트라 "지표 칩" 인상도 줌
  - `apps/viewer-expo/src/components/SetupGuideCard.tsx:31` — 가이드 제목이 `sectionTitle`(12px muted)로, 본문 단계명 `stepName`(14px, hub-styles.ts:265-268)보다 작음
- **영향**: "여러 창으로 PC 화면 보기"라는 제품 핵심이 ①처음 화면에서 스탠바이 히어로 카드 아래 장식 칩으로만 존재. 첫 화면 위계(축 ①) 약화 + 템플릿 흔적(AI-티).
- **수정안**:
  1. `FeatureCardsGrid`를 카드 그리드에서 **정의형 행 2줄**(제목 600 + 설명 1줄, 행 사이 1px 구분선 — design.md §3 "정의형 행" 패턴의 RN판)로 강등. 아이콘 박스는 제거하거나 16px 인라인 아이콘으로 축소.
  2. 가이드 제목을 `typography.base`(15) 600 + `textPrimary`로 상향해 단계명(14)과 위계 복원.
- **난이도**: 소~중 / **임팩트: 중**

### P4. 게이트·상태 신호의 명확성 — 무스타일 재시도 버튼, 중립색 상태 점, 경계성 pill

- **근거**:
  - `apps/viewer-expo/app/pairing.tsx:712-716` — 카메라 권한 오류 카드의 재시도가 `<Pressable onPress=…><Text>{t.common.retry}</Text></Pressable>` **스타일 완전 부재**(기본 14px, 탭 피드백 없음, secondaryActionBtn 등 기존 버튼 스타일과 단절). 보안 게이트(페어링 진입 수단)의 복구 동작이 UI에서 읽히지 않음
  - `apps/viewer-expo/app/pairing.tsx:644-645, 761-766` — `hostDot`이 `colors.textPrimary`(중립)를 상태 점처럼 렌더. 상태색(`statusLive` 등)은 "살아있는 상태"의 예약 신호인데 중립 점이 같은 어휘를 써서 신호 대역 오염
  - `apps/viewer-expo/src/components/hub-styles.ts:115-125` — `badgeSuccess`: 배경+보더+radius 10 pill형 크롬에 "연결 가능" 상태 텍스트. `:45-58` `langToggleBtn`: radius 17 pill. design.md G1의 판정법("pill 없이 읽어도 자연스러운가") 적용 시 둘 다 텍스트+도트만으로 충분
- **영향**: 축 ③ — 페어링·연결 상태가 사용자에게 일관된 어휘로 읽히지 않는다. 특히 권한 오류 재시도는 보안 흐름의 정체 구간이 될 수 있음.
- **수정안**:
  1. 재시도 버튼에 `secondaryActionBtn`/`secondaryActionText` 스타일 적용(컴포넌트 간 스타일 공유 또는 pairing 로컬 동등 스타일), `accessibilityRole="button"` 부여.
  2. `hostDot`은 `borderSubtle` 계열 장식 마커로 바꾸거나 제거(주소 텍스트만으로 충분). 상태색은 `statusLive/Warning/Danger`만 사용.
  3. `badgeSuccess`는 배경·보더 제거하고 `dotSuccess`(6px 녹색 점)+텍스트 유지로 강등. `langToggle`은 pill 대신 1px 보더 라운드(radius 8) 수준으로 완화.
- **난이도**: 소(1·2) ~ 소(3) / **임팩트: 중**

### P5. 가로(landscape)·넓은 패널에서 콘텐츠 최대폭 캡 부재

- **근거**:
  - `apps/viewer-expo/app.config.ts:25` — `orientation: "default"` (회전 허용 확정)
  - `apps/viewer-expo/app/index.tsx` + `hub-styles.ts:17-23` — `content`: paddingHorizontal 20 뿐, `maxWidth` 캡 없음. 가로 폰(~800dp)에서 hero 카드·버튼 행이 화면 전폭으로 늘어남
  - 완화 요인(잘 되어 있는 부분): `panel-density.ts:19-27`이 600→1500dp 선형 스케일로 폰트·터치 타깃을 확대하고, `app/index.tsx:218` SafeAreaView 4변, `app/catalog.tsx:1136`/`host.tsx:543`/`pairing.tsx:585`에서 `useWindowDimensions` 대응은 이미 갖춰짐
- **영향**: 가로 폰·분할 화면(축 ⑤)에서 터치 도달 거리 증가, hero 카드의 좌우 여백 붕괴. 기능 결함은 아니지만 "폰에서 들어 둘 화면"의 착용감 훼손.
- **수정안**: 각 화면 `contentContainerStyle`에 `maxWidth: 640`(hub/pairing)·`840`(catalog/host 패널 목록) + `alignSelf: "center"` 캡 추가. 밀도 스케일 로직은 그대로(스케일은 폭 기준이므로 캡과 독립 동작).
- **난이도**: 소 / **임팩트: 중**

### P6. (참고) 기타 소소 항목

- `apps/viewer-expo/app/host.tsx:911`, `hub-styles.ts:231` — `textTransform: "uppercase"`는 한국어 카피에는 무의미하고 영문 라벨에서만 "템플릿 냄새". 한국어 전용 라벨이면 제거 검토.
- `apps/viewer-expo/app/host.tsx:1040`, `pairing.tsx:960,981` — `textAlign: "center"`는 OTP 박스·빈 상태 안에서만 사용 중으로 design.md G5 위반 아님(현행 유지).
- `global.css:1-2` — tailwind/uniwind import만 존재, 폰트 선언 없음. FONT-DECISION §3.3과 정합(변경 불필요).
- 승인 게이트 스트림 입력 승인 UI(`use-stream-controller.ts` 경유 catalog)는 이번 정적 감사에서 구조 위반 없음 — 다만 12px 다용도 문제(P1)의 영향권.

---

## 3. 상위 5개 요약 (구현 태스크 반환용)

| # | 이슈 | 근거(대표) | 난이도 | 임팩트 |
|---|---|---|---|---|
| 1 | 다크 저대비 텍스트(textMuted 3.8:1·textDim 2.4:1) + 11~12px 미세 타이포 | `tokens.ts:53-57`, `hub-styles.ts:132-135,227-233`, `catalog-styles.ts:51` | 소~중 | **대** |
| 2 | 타이포 토큰(`typography`) 미적용 — 하드코딩+700 남용, 위계 토큰화 필요 | `theme.ts:11`, `hub-styles.ts` 전체, `tokens.ts:96-129` | 중 | 중~대 |
| 3 | 첫 화면 위계: G2 균일 feature 카드 + 가이드 제목(12px) < 본문(14px) 역전 | `FeatureCardsGrid.tsx:17-39`, `SetupGuideCard.tsx:31` | 소~중 | 중 |
| 4 | 게이트 UI: 권한 오류 재시도 무스타일, hostDot 중립색 상태 점, pill 배지 경계성 | `pairing.tsx:712-716,761-766`, `hub-styles.ts:45-58,115-125` | 소 | 중 |
| 5 | 가로·분할 화면에서 콘텐츠 최대폭 캡 부재 | `app.config.ts:25`, `hub-styles.ts:17-23` | 소 | 중 |

**폰트 전달 사항(재강조)**: viewer-expo는 시스템 기본 폰트 유지(커스텀 번들 없음), `monospace` 사용처는 FONT-DECISION §3.3 허용 예외 그대로 유지. 위계 개선은 토큰화(#2)로만 수행한다.

## 4. 제약 준수 확인

- 이 감사는 시각·위계만 다루며, 세션/스트림/페어링 로직(`session.ts`, `launch-stream.ts`, `pairing.ts`의 워크플로 등)과 네이티브 브리지(`StreamLauncher`, rustra-bridge) 동작 변경 제안은 포함하지 않는다.
- `pairing.tsx` 재시도 버튼 수정안(#4-1)은 스타일·접근성 속성만 추가하고 `handleQrScanned`/`PairingWorkflow` 로직은 그대로 둔다.
- P1의 `ui-tokens` 색상 변경은 desktop과 공유 토큰이므로, 구현 시 `apps/host-desktop/DESIGN-REVIEW.md`의 해당 지적과 상충하지 않는지 먼저 대조할 것.

## 5. 미해결 리스크

- 대비 수치는 토큰 값 기반의 계산값(근사)이며, 실기기 스크린 캘리브레이션 검증은 기기 실측이 필요해 이번 정적 감사 범위 밖이다.
- `panel-density` 스케일이 `width/height`도 확대하므로(P5에서 maxWidth 캡 추가 시) 캡 값 자체는 스케일링 대상에서 제외할지 구현 단계에서 확인 필요(`SCALED_PROPERTIES`에 `maxWidth` 포함 — `panel-density.ts:60`).
- `hubAutoAdvancedOnce`(`app/index.tsx:30,87-93`)의 400ms 자동 전환은 기능 동작이므로 이번 감사에서 판정하지 않았다. 첫 화면 위계 관점에서 "연결 직후 화면이 사용자 조작 없이 사라지는" 체감 이슈의 여지는 남으나, 로직 불변 제약상 기록만 남긴다.
