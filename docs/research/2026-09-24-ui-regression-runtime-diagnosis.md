# UI 회귀 런타임 실패 진단 — `useState` null pageerror 근본 원인

- 날짜: 2026-09-24 (UTC 기준, KST 실행은 09-25 자정)
- 커밋: `7a0b216` (`test(ui): 삭제된 SourceGrantEditor 참조 파일 제거 누락분 반영`)
- 범위: `bun run test:ui` 런타임 실패 진단 (수정 없음, 문서만 작성)
- 환경: bun 1.4.1, playwright-core(repo node_modules), Chromium 1243 "Google Chrome for Testing", `/tmp/leftcar-task4-ui`(00:21 빌드, 현재 소스와 빌드 스크립트 동일 상태)
- 이전 루프(cl: c39355c5)에서 기각된 가설: entry.js "React 두 벌" — **entry.js에는 한 벌 맞음**(하단 재확인). 단, 그 결론을 catalog/host/hub-connect/camera 번들에까지 확장 적용하는 것은 오류였음.

## 결과 요약

**확정 원인 (useState pageerror):** `tools/ui-regression/build.mjs`의 4개 빌드(catalog·camera·host·hub-connect)에 `reactPlugin`이 누락되어, native 해석이 React를 물리적으로 다른 두 경로(`<repo>/node_modules/react` 실플러 디렉터리 vs `apps/viewer-expo/node_modules/react` 심볼릭 링크→`node_modules/.bun/react@19.2.3` 캐논 경로)로 각각 번들링했다. react-dom은 ①번 사본으로 렌더하며 dispatcher를 ①번 `ReactSharedInternals.H`에 설정하고, 앱 컴포넌트(`useCatalogModel`, `LanguageProvider`)는 ②번 사본의 `useState`→`resolveDispatcher()`를 읽어 `.current === null` → `Cannot read properties of null (reading 'useState')`.

**수정 지점 (최소 수정안):**
1. `tools/ui-regression/build.mjs:49`(catalog), `:76`(camera), `:83`(host), `:93`(hub-connect) — 각 `plugins:` 배열에 `reactPlugin` 추가 (entry/extended-display/extension-viewer/pairing-grants 빌드와 동일 패턴).
2. 별도 실패 클래스(타임아웃, pageerror 없음)로 확정된 픽스처 계약 불일치 — 후술 "실패 분류" 및 수정안 2·3.

---

## 1. 재현 및 스택 수집 방법

`run.cjs`는 `page.on("pageerror", e => errors.push(String(e)))`로 메시지만 수집하므로 스택이 사라진다. `run.cjs`를 `/tmp/ui-diag/run-stack.cjs`로 복사해 pageerror 핸들러가 `error.stack` 전체를 즉시 출력하도록 패치 후 동일 플로우 실행:

```bash
cd tools/ui-regression
PLAYWRIGHT_CORE_PATH=$PWD/../../node_modules/playwright-core \
CHROMIUM_PATH="$HOME/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing" \
node /tmp/ui-diag/run-stack.cjs
```

결과(전체 12개 pageerror, 모두 동일 스택): index.html 케이스 3개 PASS, dashboard 케이스는 pageerror 없이 클릭 30s 타임아웃, catalog.html 로딩 케이스 12건(대표 1건 + presentation/audio 변형)이 즉시 pageerror 후 `window.model` 미설정으로 30s 타임아웃. 타임스탬프가 케이스 시작 직후(30초 간격 나열)이므로 catalog 크래시는 **상호작용이 아니라 첫 마운트 렌더** 시점에 발생한다(이전 루프의 "상호작용 시점" 서술은 index.html 기준 관찰이었고, catalog에서는 마운트 즉시임).

## 2. pageerror 전체 스택 (catalog.js, 12건 모두 동일)

```
Uncaught TypeError: Cannot read properties of null (reading 'useState')
    at exports.useState        (file:///tmp/leftcar-task4-ui/catalog.js:18144:34)
    at useCatalogModel         (file:///tmp/leftcar-task4-ui/catalog.js:29261:44)
    at Catalog                 (file:///tmp/leftcar-task4-ui/catalog.js:29969:18)
    at react_stack_bottom_frame(catalog.js:15378:19)
    at renderWithHooks         (catalog.js:5905:23)
    at updateFunctionComponent (catalog.js:7329:20)
    at beginWork / runWithFiberInDEV / performUnitOfWork / workLoopSync /
       renderRootSync / performWorkOnRoot / performWorkOnRootViaSchedulerTask /
       performWorkUntilDeadline
```

동일 크래시가 다른 번들에서도 확인됨(첫 pageerror, 같은 원리 — 앱 컴포넌트 프레임만 다름):

| 번들 | useState 프레임 | 크래시 컴포넌트 |
|---|---|---|
| host.js | host.js:17722 | `LanguageProvider` (host.js:20570) |
| hub-connect.js | hub-connect.js:17722 | `LanguageProvider` (hub-connect.js:24476) |
| camera.js | camera.js:17709 | `LanguageProvider` (camera.js:24151) |

## 3. 스택 역매핑 (번들 주석 → 원본 모듈)

catalog.js 기준 역매핑(`// <path>` 번들 주석과 팩토리 경계 기준):

| 번들 위치 | 소속 팩토리 / 주석 경로 | 원본 모듈 | React 사본 |
|---|---|---|---|
| :5905 `renderWithHooks` | `require_react_dom_client_development` (1349–16916) | `node_modules/react-dom/cjs/react-dom-client.development.js` | **사본 ①** `node_modules/react` (주석 :67, :890 — `require_react_development`:68, `require_react`:891) |
| :14179 `var React = require_react(), … ReactSharedInternals = React.__CLIENT_INTERNALS…` | react-dom-client 내부 | react-dom-client.development.js | 사본 ① |
| :18144 `exports.useState` | `require_react_development2` (17336–18158) | `node_modules/.bun/react@19.2.3/node_modules/react/cjs/react.development.js` (주석 :17335) | **사본 ②** `.bun/react@19.2.3` (주석 :17335, :18158 — `require_react2`:18159) |
| :29261 `useCatalogModel` | 주석 :29256 | `apps/viewer-expo/src/use-catalog-model.ts` | 사본 ② (`import_react3 = __toESM(require_react2())` :28121) |
| :29969 `Catalog` | 주석 :29958 | `tools/ui-regression/catalog.jsx` | **사본 ①** (`import_react4 = __toESM(require_react())` :18612) |

host.js의 `LanguageProvider`는 주석 :20552 → `apps/viewer-expo/src/i18n.ts`이며 `import_react3.useState` 사용(사본 ②), react-dom-client(:5898 `renderWithHooks`)는 사본 ① — catalog.js와 동일 구조.

**발화 지점(번들 내 실문):** catalog.js:5906 근처에서 react-dom(사본 ①)이 `ReactSharedInternals.H = … HooksDispatcherOnMountInDEV`로 **사본 ①의** internals에 dispatcher를 걸고, :18144의 `resolveDispatcher()`(사본 ②의 internals)가 이를 읽어 `null`을 반환한다. 두 개의 독립된 `ReactSharedInternals` 객체가 존재하는 것이 크래시의 직접 기제다.

## 4. 왜 두 벌이 번들링되었나 — build.mjs 결함

`tools/ui-regression/build.mjs`에서 `reactPlugin`(react·react-dom·jsx-runtime을 `node_modules/.bun/react@19.2.3` 실경로로 고정)은 4개 빌드에만 전달되지 않았다:

| 빌드 (build.mjs 행) | reactPlugin | 결과 |
|---|---|---|
| entry (:22) | ✅ | 단일 사본(.bun 경로만 존재, grep 확인) — index.html 케이스 통과 |
| extended-display (:25), extension-viewer (:28), pairing-grants (:73) | ✅ | pairing-grants.js 단일 사본 확인 |
| **catalog "controlled-device-io" (:49)** | ❌ | **2벌** (`node_modules/react` + `.bun/react@19.2.3`) |
| **camera "camera-os-only" (:76)** | ❌ | **2벌** |
| **host "host-os-and-transport-only" (:83)** | ❌ | **2벌** |
| **hub-connect "hub-os-and-transport-only" (:93)** | ❌ | **2벌** |

reactPlugin이 없으면 Bun의 native 해석이 임포터 위치에 따라 다른 물리 경로에 도달한다:

- `tools/ui-regression/catalog.jsx` → 상위 탐색 → `<repo>/node_modules/react`, `<repo>/node_modules/react-dom`, `<repo>/node_modules/@tanstack/react-query` — **모두 실플러 디렉터리**(2026-08-25자, 버전 19.2.3 동일). 주석도 `node_modules/react/...`로 기록.
- `apps/viewer-expo/src/use-catalog-model.ts` → `apps/viewer-expo/node_modules/react` — **심볼릭 링크**(`../../../node_modules/.bun/react@19.2.3/...`) → 캐논 경로 `.bun/react@19.2.3/...`로 주석 기록.

Bun은 해석 경로 문자열이 다르면 같은 소스라도 별 모듈로 번들링하며, `__commonJS` 팩토리가 2벌 생성된다(`require_react` vs `require_react2`). 동일 현상이 `@tanstack/react-query`에도 발생 — catalog.js에 QueryClientProvider가 2벌(루트 실디렉터 사본 :20463, `.bun` 캐논 사본).

참고: 이전 루프의 "'React 두 벌' 가설 기각"은 entry.js 한정으로 **옳았다**(entry.js는 reactPlugin 덕에 단일 사본). 기각이 전 번들에 성립한다고 확장한 것이 오류 — catalog/host/hub-connect/camera에서는 실제로 2벌이다.

## 5. 최소 재현으로 픽스 효과 검증

`reactPlugin` + `use-catalog-model.ts`(viewer-io/tcp-io/tanstack 스텁 포함)만 임포트하는 1-파일 엔트리를 동일 옵션으로 Bun.build한 결과: 출력 내 react 관련 주석이 **전부 `.bun/react@19.2.3` 및 `.bun/react-dom@19.2.3` 단일 경로**로 수렴(react-dom 내부 `require("react")`, tanstack의 react까지 플러그인이 가로챔). 즉 누락된 4개 빌드에 `reactPlugin`을 넣는 것만으로 단일 사본이 보장된다. (`/tmp/ui-diag/buntest/b.mjs`, 저장소 밖.)

## 6. 후보별 확인/기각

| 후보 | 판정 | 근거 |
|---|---|---|
| 순환 import로 인한 null 바인딩 | **기각** | 크래시는 모듈 바인딩 null이 아니라 React `resolveDispatcher()`가 dispatcher null을 읽은 것. 스택이 `renderWithHooks→Catalog→useCatalogModel→useState`로 정상 완결되며, 모듈 초기화는 전부 완료된 후 첫 렌더에서 발생. |
| 앱 소스의 조건부/지연 import | **기각** | 경로에 dynamic import 없음. 크래시는 `page.goto` 직후 첫 마운트 렌더에서 발생(상호작용 불필요). index.html(entry.js)은 동일 앱 훅(`usePrivacySettings`)이 마운트·상호작용 모두 통과. |
| **reactPlugin 인터롭** | **확인 (근본 원인)** | §4: 4개 빌드에 플러그인 누락 → 이중 사본(§3 역매핑) → §5 재현으로 픽스 검증. 정확히는 "인터롭 결함"이 아니라 "플러그인 미적용 빌드가 있었다"가 결함. |
| 픽스처 io 모킹과 앱 코드의 계약 불일치 | **확인 (제2 실패 클래스 — pageerror 아님)** | 하단 §7. 타임아웃만 유발하고 useState 크래시와는 무관. |

## 7. 실패 분류 (9스위이트 전체)

**A 클래스 — 이중 React 크래시 (`useState` pageerror, 마운트 즉시)**
- run.cjs의 catalog 케이스 12건 (catalog.js)
- host-lifetime.cjs 전체 (host.js — `input` 대기 3s 타임아웃으로 표면화)
- hub-connect.cjs 전체 (hub-connect.js)
- retained.cjs의 camera 케이스 1건 (camera.js)

**B 클래스 — 픽스처·앱 UI 계약 불일치 (렌더는 되나 기대 셀렉터 부재 → 타임아웃, pageerror 0건)**

앱 UI는 9월 중순 리팩터링(커밋 `17f8bcf` 설정 모달 간소화, `301ae5d` PairingPanel 재작성 — 화면 접근 카운터 제거, 415줄 변경)으로 바뀌었고 스위트 일부가 구 계약을 Still 기다린다:

1. run.cjs dashboard 케이스: `getByRole("button", { name: /Privacy Curtain/ })`를 대시보드에서 직접 기대(run.cjs:80). 실제 `index.html?dashboard` DOM 버튼 목록 = 한국어 토글, Theme, Help, Host Settings, Grant Permission, Open Settings, Generate Pairing Code, Computer Address, Remote Input, Settings — **Privacy Curtain 없음**. 인터랙티브 커튼 토글은 Settings 모달 항목으로 이동(`apps/host-desktop/src/modals/DashboardModals.tsx:60-77`, 라벨 `t.host.privacyCurtainLabel` = "Privacy Curtain", `packages/ui-tokens/src/i18n.ts:946`), 푸터는 수동 표시 pill만 남음(`apps/host-desktop/src/components/DashboardFooter.tsx:77-80`).
2. pairing-grants.cjs / pairing-revoke.cjs / pairing-incarnation.cjs / pairing-ordering.cjs: `화면 접근: 1`, `화면 접근 모두 제거`, `Host에서 화면 접근 검토가 필요합니다` 등 구 UI 텍스트 대기. 현재 `apps/host-desktop/src` 전역에 `화면 접근` 문자열 부존재(grep 0건). 신 UI는 `.device-row-item` + `PairedDeviceRow` 요약(`apps/host-desktop/src/pairing/PairedDeviceRow.tsx:20-26`, `packages/ui-tokens/src/i18n.ts:392-394`: `승인 대기`/`화면 {count}개`/`화면 권한 없음`)와 `허용`/`거절` 결정 버튼. **retained.cjs의 pairing 케이스(허용/거절)는 신 셀렉터로 PASS** — 페어링 픽스처 페이지와 io 모킹 자체는 건강하며 구 스위트 4종의 DOM 기대만 낡음. (단, `set_source_grants` 저장·revoke·incarnation 시맨틱을 신 UI 흐름에 재사상하는 작업이 수반되므로 단순 셀렉터 치환 이상의 수정 필요.)

## 8. 수정안 (코드 미변경 — 제안)

**수정안 1 (A 클래스, 최소·권장):** `tools/ui-regression/build.mjs`의 4개 빌드 plugins 배열 선두에 `reactPlugin` 추가.
- `:49` `plugins: [reactPlugin, { name: "controlled-device-io", … }]`
- `:76` `plugins: [reactPlugin, { name: "camera-os-only", … }]`
- `:83` `plugins: [reactPlugin, { name: "host-os-and-transport-only", … }]`
- `:93` `plugins: [reactPlugin, { name: "hub-os-and-transport-only", … }]`
- 근거: §5 재현(플러그인 적용 시 react-dom·tanstack 내부 react까지 단일 `.bun` 경로 수렴), entry/pairing 빌드의 선행 사례. 앱 소스(`apps/**`) 변경 0 — 결함 계층이 픽스처 빌드이므로 계약상 올바른 지점.

**수정안 1의 대안:**
- (a) 낡은 루트 실플러 디렉터 `node_modules/{react,react-dom,@tanstack}` 제거(재설치) — 환경 수복일 뿐 임포터별 경로 분기(tools vs apps)는 남고 재발 가능. 단독 채택 부적합.
- (b) Bun.build `alias`로 react 고정 — 플러그인과 동일 효과이나 기존 코드 패턴과 불일치. 채택 이유 없음.

**수정안 2 (B-1):** `tools/ui-regression/run.cjs` dashboard 케이스(:77~)에서 설정 모달을 먼저 열도록 수정 — 예: `page.getByRole("button", { name: "Host Settings" }).click()` 후 커튼 토글(`"Privacy Curtain"`)·Retry 상호작용. 앱 소스 되돌림(토글을 대시보드에 재노출)은 테스트를 위한 UX 번복이므로 금지.

**수정안 3 (B-2):** pairing-grants/revoke/incarnation/ordering 4종의 DOM 기대를 신 UI 계약으로 갱신(retained.cjs 패턴 준용): 펜딩 오퍼 `Synthetic pending viewer` + `허용`/`거절`, 그랜트 요약 `화면 N개`/`승인 대기`/`화면 권한 없음`, 오류 텍스트 `연결 상태를 확인하지 못했습니다`. io 모킹 명령(list_paired_devices / set_source_grants / revoke_paired_device / list_pending_pairings / approve·reject_pending_pairing)은 현행 앱과 계속 일치하여 유지 가능.

## 9. 검증되지 않은 사항·리스크

- 수정안 1~3 적용 후 `bun run test:ui` 9스위이트 전체 통과는 **본 진단에서 수행하지 않음**(본 과업은 코드 변경 금지). 특히 수정안 3은 각 스위트가 검증하던 시맨틱(저장 지연·재시도·stale 스냅샷)을 신 UI 흐름에 정확히 재사상해야 하며, Settings 모달 안 토글의 `aria-busy`/Retry 접근성 이름은 수정 단계에서 실제 DOM으로 확인 필요.
- catalog 빌드에 reactPlugin 추가 시 `@tanstack/react-query` 이중 사본(§4)도 함께 수렴하는지는 수정 후 번들 주석 재검 권장(§5 재현에서는 수렴 확인).
- `/tmp/leftcar-task4-ui`는 00:21 생성분(7a0b216 이전 9분)이나, 해당 커밋은 파일 삭제만 포함하므로 번들 내용에 영향 없음을 확인했다.
