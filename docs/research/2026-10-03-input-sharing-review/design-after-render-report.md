# 시작 실패 표현 수정 후 isolated render

2026-10-03. 두 가지 변경 오류 표현만 별도 fixture로 만들었다. 제품 소스·기기·설치·권한·커밋 변경, 새 광범위 감사, 캡처 없음. 기존 `/private/tmp/leftcar-km-design-qa`는 수정하지 않았다.

- **KO/dark, helperUnavailable · androidPreparation:** <http://127.0.0.1:43188/viewer.html?case=helper-unavailable>
- **EN/light, generic startFailed · nativeHandshake:** <http://127.0.0.1:43188/viewer.html?case=native-handshake>

파일 `/private/tmp/leftcar-km-design-qa-after`, 서버 127.0.0.1:43188 전용 실행 중(session `27957`). 내부 content frame은 두 경우 모두 **360×800 CSS px**. Root가 이 두 페이지의 변경 오류/단계/복구 문구 줄바꿈을 필요한 범위로 관찰할 수 있다. fixture toolbar는 캡처에 섞이지 않도록 제외했다.

## 실제 입력과 frozen 소스

실제 `InputSharingScreen` default export와 내부 컴포넌트·primitives·tokens·compiled CSS를 import한다. 이전과 동일한 repository RN DOM adapter 및 hook/language controlled boundary만 사용한다. 현재 production presenter는 `errorStage`가 아니라 **`model.failure?.stage`**를 읽으므로 입력도 `error: issue, failure: { issue, stage }`를 사용했다. OFF, busy=false, 양방향 동의=true, arrangement=right, 권한=true, capability 아직 미확인=false, reason=null이다. 화면 문구를 재작성하지 않았다.

`start-diagnosis-manifest.json`의 **9개 파일 모두** 복사 당시 afterSha256과 일치했다. 새 imported `src/km-failure.ts`도 포함했다. 변경되지 않은 UI 렌더 의존성/CSS/adapter는 기존 byte-verified snapshot에서 복사했다. dependency node_modules만 공유하고 production source는 모두 byte copy다. manifest에 9개 일치 결과와 전체 frozen dependency hash가 있다.

| 핵심 frozen path | SHA-256 |
| --- | --- |
| `apps/viewer-expo/app/input-sharing.tsx` | `1b58a7a090ebbd26080726bb32a81c5e83fbaa4711e86454b3850495023bbc0e` |
| `apps/viewer-expo/src/use-km-model.ts` | `43581eda4580519e394245f4aa013c6a0d6b9e1f9feb0fe32d00b0b8a22a730d` |
| `apps/viewer-expo/src/km-session.ts` | `13f70821ffd6816ddf9f452503491ba4757edd019b0a0f2ac72f2700a5184ed7` |
| `apps/viewer-expo/src/km-failure.ts` | `3a0950ce5a2e5731049a37566860a7e6641662df236bb8b0e06a1a0b6536ef66` |
| `packages/ui-tokens/src/i18n.ts` | `35870662567597a1d8c698c352d8f06bb73d2c76dfd4eac5abf2400d22e15033` |

전체: `/private/tmp/leftcar-km-design-qa-after/manifest.json`; 출력 hash: `build-receipt.json`; SSR: `render-receipt.json`. 이 freeze는 **렌더 의존성 snapshot**만 뜻한다. 병행 cancel 수정이나 전체 application candidate를 freeze/검증한 것으로 해석하지 않는다.

## scoped 확인과 남은 중복

- `bun /private/tmp/leftcar-km-design-qa-after/build.mjs`: exit 0, frozen hash 확인, 실제 CSS/browser bundle 및 두 페이지 생성.
- `bun /private/tmp/leftcar-km-design-qa-after/after-render-check.mjs`: exit 0. 두 경우에 실제 issue/단계 문구, generic nativeHandshake의 해당 recovery 문구, Start와 동의 switch 두 개를 SSR로 확인했다. HTTP 페이지/JS/CSS는 모두 200.
- Root가 요청한 **cleanupFailed 중복만** 추가 SSR 검사했다. 현재 frozen 화면에서 `nativeStateLabel`의 `cleanupFailed` branch와 `SharingFailure` Notice가 모두 완전한 `t.km.stopFailed` 문장을 출력하여 **동일 긴 실패 설명이 2회** 나타난다. 새 `cleanupRecovery`도 별도로 나타난다. 따라서 pre-patch capture에서 보였던 중복은 현재 F2 presenter에서도 남아 있다. 기존 의미를 유지한 짧은 상태 라벨 + 한 번의 상세 실패/복구 설명으로 정리할 후보지만 제품을 수정하지 않았다. 이 검사는 세 번째 캡처 페이지를 추가하지 않았다.

SSR/build는 실제 browser pixel/레이아웃·native Uniwind·폰트 배율·safe-area·navigation chrome·TalkBack/VoiceOver·실기기 물리 입력/원격 종료 증거가 아니다. callback은 no-op이고 state는 고정값이며 오류를 실제 발생시킨 runtime 검증도 아니다. 이 보고서는 현재 제품 React Doctor/전체 테스트/패키지 통과를 새로 주장하지 않는다.
