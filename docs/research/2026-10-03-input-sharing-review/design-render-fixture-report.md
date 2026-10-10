# 입력 공유 production render fixture

2026-10-03. 기존 디자인 감사의 시각 확인용 일회성 fixture만 만들었다. 제품 소스·7개 발견·설치·기기·권한·패키지·커밋 변경 없음.

**서버:** <http://127.0.0.1:43187/> — 로컬 127.0.0.1 전용, Bun 서버 실행 중(session `20925`). 파일: `/private/tmp/leftcar-km-design-qa`. 각 페이지 위의 Surface/Size/Language/Theme/State 선택과 Render로 **하나의 production 표면만** 바꾼다. CUA 브라우저 viewport를 변경한다고 가정하지 않고 내부 content frame의 폭/높이만 고정한다.

## Root의 목적별 시작 URL

- Host 좁은 폭, 실제 OFF 경고 조합, KO/light: <http://127.0.0.1:43187/host.html?platform=host&size=narrow&lang=ko&theme=light&state=off-warning>
- Android compact, 긴 실패 문구, EN/light: <http://127.0.0.1:43187/viewer.html?platform=viewer&size=compact&lang=en&theme=light&state=long-failure>
- Android compact, 정리 실패/복구 컨트롤, KO/dark: <http://127.0.0.1:43187/viewer.html?platform=viewer&size=compact&lang=ko&theme=dark&state=cleanupFailed>
- Android tablet, 현재 owner/선택, EN/dark: <http://127.0.0.1:43187/viewer.html?platform=viewer&size=tablet&lang=en&theme=dark&state=macToAndroid>

필요하면 같은 컨트롤에서 armed/각 owner/정상 OFF를 바꾼다. 112개 링크는 탐색 목록일 뿐 캡처 요구량이 아니다. 목적별 2–4회 캡처와 AX 관찰이면 충분하다. 아직 감사자가 브라우저를 관찰/캡처하지 않았으므로 **픽셀 배치·clipping·대비·키보드 동작 통과를 주장하지 않는다.**

## 실제 사용한 production 코드

- Host: frozen `KmConsentPanel.tsx`의 실제 exported `KmConsentView`, 실제 Host primitives·shared recipes/tokens/i18n·compiled Host CSS.
- Android: 이 snapshot에는 `InputSharingView` export가 없다. 실제 default `InputSharingScreen`을 그대로 import하고 **useKmModel과 useAppLanguage 입력만** controlled boundary로 교체했다. 내부 SharingStatus/동의/좌우/SessionActions/권한 컴포넌트와 실제 RN primitives는 그대로다.
- RN 렌더러: 기존 `tools/ui-regression/camera-io.jsx`, `viewer-io.js`, `uniwind-io.jsx`를 그대로 복사해서 사용하는 repository DOM adapter. `useColorScheme`만 query theme를 읽게 교체했다. Tauri invoke는 실제 작업을 못 하도록 reject stub이며 view callback은 no-op이다.
- CSS: snapshot의 실제 `tools/ui-regression/styles.mjs`/`compileUiStyles` 패턴과 `apps/host-desktop/src/index.css`, `apps/viewer-expo/global.css`, generated theme CSS, shared role classes를 사용했다. 제품 markup/라벨/토큰을 별도로 재작성하지 않았다. fixture toolbar/frame만 외부 scaffolding이다.

## 고정 snapshot과 hash

원본: `/private/tmp/leftcar-km-20261003`, HEAD `72ff99000ab1981f9773a0b90aad3ee7ca42faad`의 당시 dirty 소스. 별도 worker의 시작 실패 reason 수정 **이전 audited 구현**이다. 23개 파일을 `/private/tmp/leftcar-km-design-qa/source`에 byte copy하고 매 빌드 시 SHA-256을 확인한다. 현재 변경을 따라가는 symlink source가 아니다. dependencies만 기존 node_modules를 사용한다.

| frozen production file | SHA-256 |
| --- | --- |
| `apps/viewer-expo/app/input-sharing.tsx` | `826f883cb9f48c37f24e81b536b25b8606a23cc588b76bf6aa69a15b9ce9962a` |
| `apps/viewer-expo/src/ui/primitives.tsx` | `6a916c67016858e60e779ca5ff0f3a09c60d9fefdaa470a118ca8e72a56ce3d8` |
| `apps/host-desktop/src/pairing/KmConsentPanel.tsx` | `14b031d4a0ba45a480f757175aeb4ba9e1bb89c393985bade64671070d426884` |
| `apps/host-desktop/src/ui/primitives.tsx` | `a4ba862d20dc3ca97ef74ff74d92bb730bdec17ad872b4e7384c0daa43d5e99a` |
| `packages/ui-tokens/src/i18n.ts` | `7d5b624835d61cd7eab4149cc735a3bb518d51528ee6d5a909dd8e949f8dda60` |
| `packages/ui-tokens/src/recipes.ts` | `87f3702c3316f214a9195854aa108b9fdea1de946d99ae416e2e42e311b412da` |
| `packages/ui-tokens/src/tokens.ts` | `716e3f0eb53681b613bd81047ebbcd8cb357f884a01cc7cea381791e6e27467e` |

전체 파일/hash: `/private/tmp/leftcar-km-design-qa/manifest.json`. build output hash: `build-receipt.json`. SSR 확인: `render-receipt.json`.

## 입력 조합과 dimensions

| surface | content frame | controlled states |
| --- | --- | --- |
| Host narrow | 360×800 CSS px, 주변 padding 16, panel 폭 328 | off-ready, off-warning, armed, macToAndroid, androidToMac, long-failure |
| Host wide | 720×800 CSS px, 주변 padding 16, panel 폭 688 | 동일 |
| Android compact | 360×800 CSS px | off-ready, setup, pending, armed, macToAndroid, androidToMac, cleanupFailed, long-failure |
| Android tablet | 1024×768 CSS px | 동일 |

모든 조합에 KO/EN 및 light/dark를 적용한다. Viewer theme는 실제 CSS의 `.light/.dark` variant를 선택하며 Host는 실제 `[data-theme]` 속성을 선택한다. body/viewport의 바깥 fixture overflow를 허용하고 production screen의 ScrollView 자체는 유지한다.

정상 OFF/active 기본 입력: 두 방향 승인 true, arrangement right, Android 권한 세 개 true, Host Mac 권한 둘 true. `off-warning`은 현재 실제 AX와 같은 OFF+blockedReason이며 문구는 실제 generic hostBlocked이다. `setup`은 Android 권한 false, `pending`은 preparing+busy, `long-failure`는 Android 실제 startFailed 또는 Host 실제 saveFailed 문구다. 임의 긴 문구/새 UX를 만들어 길이를 부풀리지 않았다.

**Host는 cleanupFailed contract가 없으므로 이 상태를 흉내내지 않는다.** Host state를 off로 만들고 새로운 종료 오류를 붙이는 가짜 구현도 하지 않았다. cleanupFailed render는 실제 Android contract에서만 확인한다.

## 실행 증거와 제한

- `bun /private/tmp/leftcar-km-design-qa/build.mjs`: exit 0, frozen hash 확인, 112 parameterized 페이지 및 실제 compiled CSS/browser bundle 생성.
- `bun /private/tmp/leftcar-km-design-qa/render-check.mjs`: exit 0, 56 state×language×theme 조합을 production renderer로 SSR. 동의 switch 두 개, owner/armed, actual failure 문구, cleanupFailed의 Stop 유지/Start 부재 확인. size 두 개는 markup이 같으므로 SSR로 중복 검사하지 않았다.
- 목적별 HTTP 조회: index, Host/Viewer 페이지, JS/CSS 모두 200. broad passed suite를 재실행하지 않았다.
- SSR/build는 **브라우저 실행 성공 및 시각 배치 증거가 아니다.** Root CUA가 actual page/AX/console 가능 범위에서 관찰해야 한다.
- Android nav header/back/status bar, native safe-area/IME/font scaling, RN Uniwind native layout, OS cursor/physical crossing, TalkBack/VoiceOver, 실제 permissions/cleanup/remote shutdown은 재현하지 않는다. Host 전체 paired row/context/app chrome도 이 패널 fixture 범위 밖이다.
- callback은 inert이며 조회 polling/Start/Stop/동의 상태 변화 시험이 아니다. 선택/disabled 상태는 controlled props의 시각·AX 표현만 볼 수 있다. 실제 native screenshot 또는 실기기 수용 증거로 기록해서는 안 된다.
- React product source를 바꾸지 않았으므로 product React Doctor 재실행 대상 변경이 없다. 이번 isolated fixture build/SSR 결과를 제품 gate로 승격하지 않는다.
