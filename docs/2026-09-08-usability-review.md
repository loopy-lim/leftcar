# 사용성 개선 — 조사 결과와 이번 작업 범위 (2026-09-08)

뷰어 앱(Expo/Kotlin), 스트림 세션(StreamActivity), 호스트 앱(Tauri)을
사용자 흐름 관점에서 조사했다. 아키텍처가 아니라 **사용자가 겪는 마찰**만
모았고, 그중 이번에 고칠 것과 뒤로 미룰 것을 구분했다.

## 1. 발견한 불편 지점 (우선순위 순)

| # | 영역 | 문제 | 근거 |
|---|------|------|------|
| 1 | 반복 사용 | 허브 화면의 "최근 연결한 컴퓨터" 띠가 눌리면 컴퓨터 선택 화면으로 이동만 할 뿐 **직접 연결되지 않는다** | `app/index.tsx:174-189` (`onPress={openHostPicker}`) |
| 2 | 오류 전달 | 매핑되지 않은 오류가 **영어 원문 그대로** 사용자에게 노출된다 (`control request timeout` 등) | `src/control.ts:225` (`return message;`) |
| 3 | 설정 | 언어(EN/한국어) 토글 선택이 **앱 재시작마다 초기화**된다. 저장 로직이 없다 | `src/i18n.ts:33-40` (plain `useState`) |
| 4 | 제스처 | 탭·드래그·두 손가락 스크롤·길게 눌러 우클릭이 구현돼 있지만 **어디에도 안내가 없다**. 코드 주석에만 존재 | `StreamTouchGestures.kt:5-17`, 안내 UI 전무 |
| 5 | 페어링 | 진행 중 상태가 문자 그대로 `"..."`로 표시된다. 잘못된 입력 오류 자리에 입력창 제목("6자리 인증 번호 입력")이 들어간다 | `app/pairing.tsx:230`, `:219-222` |
| 6 | 세션 기본값 | FPS 오버레이가 **기본 켜짐**이고, 터치할 때마다 `SKIP/LOSS/CAP→SURF` 같은 개발자 통계 HUD가 6초간 뜬다 | `src/viewer-preferences.ts:31`, `StreamHudController.kt:298-321` |
| 7 | 재연결 | 카탈로그에서 401이 나면 페어링 화면으로 보내지만 **대상 주소를 전달하지 않아** PIN 입력이 불가능한 상태가 된다 | `src/use-catalog-model.ts:121-133` (`/pairing` params 누락) |
| 8 | 무음 실패 | 허브 화면에서 토큰이 만료되면 **아무 알림 없이** 연결 해제 상태로 바뀐다 | `app/index.tsx:58-69` |
| 9 | USB | "USB 감지됨 · 권한 허용 필요" 띠가 **아무 동작도 하지 않는** 안내로 끝난다. 권한 다이얼로그는 화면을 열 때야 뜬다 | `app/catalog.tsx:509-541` |
| 10 | 호스트 | 오류 배너의 "설정 열기" 버튼 표시 여부를 **번역된 한국어 문구(`권한`) 포함 여부**로 판단해 영어 UI에서는 버튼이 사라진다 | `apps/host-desktop/src/App.tsx:594-609` |
| 11 | 호스트 | 종료 사유가 영어 원문(`startCapture failed: …`) 그대로 한국어 UI에 노출된다 | `apps/host-desktop/src/streamTermination.ts:53-63` |
| 12 | 로케일 | 카탈로그 카드 상당수, 네이티브 계층 전체, 오류 문자열 전반이 한국어 하드코딩이라 EN 사용자가 한국어를 보게 된다 | `catalog.tsx`, `StreamActivity.kt` 등 (전수 조사는 보고서 참조) |

## 2. 이번에 고치는 것 (Spec)

선정 기준: ① 일상 사용 빈도가 높은 마찰, ② 동작이 이미 존재하는 흐름 위의
작은 수정( bounded ), ③ 검증 가능. "simple is best / 옵션 남발 금지" 원칙에
맞춰 새 설정을 하나도 추가하지 않는다.

| ID | 개선 | 완료 기준 |
|----|------|-----------|
| V1 | 허브 최근 컴퓨터 **원탭 연결** + 무음 로그아웃 시 승인 요청 알림→페어링 이동 | 띠 탭 시 곧장 카탈로그 진입(401이면 페어링 안내). 실패는 인라인 오류 |
| V2 | **언어 선택 유지**(SecureStore) | 앱 재시작 후에도 선택 유지. 감지 규칙은 순수 함수로 분리해 단위 테스트 |
| V3 | 페어링 화면 문자열 정리: `"..."`→실제 진행 문구, 제목 오용→전용 오류 문구, 대상 없음 안내 분리, 팁 박스 i18n, 하드코딩 색상(`#4a90e2`) 제거 | 자리표시자/문자열 역할 오류 없음. ui-tokens ko/en 키 쌍 추가 |
| V4 | 카탈로그 401 → 페어링 **엔드포인트 전달** | 페어링 화면이 대상 주소를 보여주고 PIN 입력 가능 |
| V5 | `formatErrorMessage`: 화면 녹화 권한 매핑 추가 + 미매칭 영어 오류를 "안내문 (원문)" 형태로 래핑. 한글 문구는 그대로 통과 | `control.test.ts` 사례 추가, 전수 통과 |
| V6 | **FPS 오버레이 기본 끄기** + 상세 통계 HUD도 같은 설정 뒤로(새 설정 추가 없음) | 신규 설치 기본값 `showFps=false`. 기존 저장값은 유지. HUD는 설정 ON일 때만 생성/노출 |
| V7 | **첫 스트림 창에 1회 제스처 안내** 오버레이(탭=클릭, 끌기=드래그, 두 손가락=스크롤, 길게 누르기=우클릭) | PopupWindow로 영상 위 표시, 확인/바깥 탭으로 닫기, 재방문 미노출 |
| V8 | USB 띠에 **"USB 권한 허용" 버튼**(기존 `requestUsb` 제어 명령 재사용) | 감지+미권한 상태에서 버튼 노출, 요청 후 상태 구독으로 갱신 |
| H1 | 호스트 오류 배너를 **종류(kind) 기반**으로 분기(번역 문구 매칭 제거) + 종료 사유 원문을 안내문 뒤 괄호로 | 영어 UI에서도 설정 열기 버튼 노출. kind 판정은 원시 오류 문자열 기준 |

### V6 상세 (설정 의미)

`showFps` 설정 하나가 "FPS 배지 + 상세 통계 HUD"를 함께 통제하는 **진단
표시** 설정이 된다. 라벨은 그대로 두고 동작만 확장 — 새 토글을 만들지
않는다. 개발용 계측이 필요하면 카탈로그의 "실제 FPS 항상 표시"를 켜면
된다.

## 3. 구현 계획 (Plan)

1. `packages/ui-tokens/src/i18n.ts` — 신규 키(ko/en 쌍): `pairingBusy`, `invalidPinError`, `pinNoTargetDesc`, `pairingTipTitle`, `pairingTipCode`, `pairingTipNetwork`, `connectingToHost`, `usbGrantAction`
2. V3 `app/pairing.tsx` 문자열/스타일 수정
3. V1 `app/index.tsx` 원탭 연결 + 401 알림 (V5 선행: `src/control.ts` 매핑)
4. V4 `src/use-catalog-model.ts` 엔드포인트 전달
5. V2 `src/i18n.ts` SecureStore 유지 + `resolvePersistedLanguage` 테스트
6. V6 `src/viewer-preferences.ts` 기본값 + `StreamHudController.kt` 통계 HUD 게이트 + `StreamActivity.kt` 기본값 동기화
7. V7 `GestureHintOverlay.kt` 신규 + `StreamActivity.onCreate` 연결 + 단위 테스트
8. V8 `app/catalog.tsx` USB 버튼
9. H1 `apps/host-desktop/src/App.tsx` + `streamTermination.ts`
10. 검증: `bun run typecheck`, `bun run test`, Android 단위 테스트(gradlew), `npx react-doctor`(100/100 게이트)

## 4. 이번에 안 고치는 것 (Backlog)

- **QR이 PIN 입력을 없애주지 않음** — QR에 offer secret이 있어도 호스트 화면의 6자리 입력이 필수다. 페어링 모델 변경(프로토콜)이 필요해 별도 설계로 미룸.
- **IME/소프트 키보드 원격 입력** — 입력 프로토콜에 텍스트 이벤트가 없다(`input_protocol.rs`). 신규 프로토콜 메시지가 필요해 별도 작업으로.
- **스트림 정지 12초 무음 프리즈** — 조기 "불안정" 표시는 렌더러 헬스 래더와 얽혀 반응성 회귀 위험이 있다. 별도 검증 과제로.
- **macOS 화면 녹화 권한, 호스트 대시보드에 미표시** — 프로젝트 자체 감사 문서(platform-permissions-audit.md #3)에 등재된 과제. shim preflight + Rust 명령 + 배너까지 3계층 작업이라 다음 단계 후보 1순위로 제안.
- **원격 입력 자동 승인** — f1a846e로 의도적으로 변경돼 기기 검증까지 완료된 설계(반대로 문서가 낡은 상태). 사용성 관점에서는 "입력 활동 표시(호스트 트레이)" 부재만 남아 있어 이 역시 다음 단계로.
- **전면 i18n(카탈로그 카드·네이티브 문자열·오류 전체)** — 범위가 크고 번역 품질 논의가 필요하다. 이번엔 V2/V3로 "선택이 저장되고 페어링 화면은 완전 번역"까지만.
- **호스트 페어링 승인 다이얼로그·엔드유저 설치 문서** — 제품 정책(PRDP-01)과 맞물려 별도 논의.

## 5. 검증 (2026-09-08 완료)

- `bun run typecheck` — 통과 (viewer + 공용 패키지; host-desktop `tsc --noEmit` 별도 통과)
- `bun run test` — **468/468 통과**. 갱신된 계약: `formatErrorMessage` 미매핑 영어 원문 래핑, `showFps` 기본값 false, 종료 사유 안내문 우선
- `bun run test:contract` / `test:architecture` — 통과
- Android `:app:testReleaseUnitTest` — 통과 (신규 `GestureHintRowsTest` 3/3 포함)
- React Doctor — **100/100**. 1차 84/100(거대 컴포넌트, effect 의존성) → 최근 호스트 띠를 `RecentHostQuickConnect` 컴포넌트로 추출, 엔드포인트를 `controlHost()` 게터로 해결 후 만점
- 기기 검증(제안): 레노보 태블릿에서 ①허브 원탭 재연결 ②첫 실행 제스처 안내 1회 표시 후 미재노출 ③기존 기기의 저장 설정(FPS 배지)이 유지되는지 확인

### 커밋 전 알림

작업 트리에 이전 세션의 미커밋 WIP(터치 제스처: `StreamTouchGestures.kt`, `StreamActivity.kt`
제스처 로직, `CaptureSession+Input.swift`)가 함께 있다. 이번 사용성 작업은 그 위에
얹혀 있고 커밋하지 않았다. 커밋 시 WIP의 `LeftcarGesture` 디버그 로그
(`StreamActivity.kt` `forwardTouchGesture`/`syncLongPressTimer`)를 먼저 정리할 것 —
기존 합의된 사전 작업이다.

### 후속 (같은 날 오후)

- 위 알림대로 `LeftcarGesture` 로그를 정리하고 제스처 WIP와 사용성 작업을 분리 커밋했다.
- 제스처 WIP의 shim 코드(`usesNaturalScrolling`)는 `NSGlobalDomain` 참조 때문에 **컴파일되지
  않던 상태**였다(`UserDefaults.globalDomain`으로 수정해 같은 커밋에 흡수). 따라서 이전 세션의
  "자연 스크롤 방향 존중"은 실기 미검증으로 봐야 한다 — §6 체크리스트 5번.
- 백로그 1순위였던 **화면 녹화 권한 대시보드 표시**를 구현했다: shim preflight export →
  Rust `get_screen_permission` → `SystemAlertBanners` 경고 배너(i18n `screenPermBannerTitle/Desc`).
  BetterDisplay 항목은 코드가 이미 f1a846e에서 제거돼 있어 문서만 정리했다.

## 6. 기기 검증 체크리스트 (사용자 수행)

사전 준비: 호스트는 `tools/dev-host-macos.zsh`로 shim과 앱을 재빌드해 `/Applications/Leftcar Host.app`에
설치한 뒤 실행한다 — 설치돼 있던 앱은 이전 dylib/바이너리라 배너와 자연 스크롤 변경이 반영되지 않는다.
뷰어는 `:app:assembleRelease` APK를 태블릿에 `adb install -r`.

| # | 항목 | 절차 | 기대 결과 |
|---|------|------|-----------|
| 1 | 허브 원탭 재연결 (V1) | 허브의 "최근 연결한 컴퓨터" 띠 탭 | 곧장 카탈로그 진입. 호스트에서 기기를 해제한 뒤 탭하면 승인 요청 알림 → 페어링 화면에 대상 주소가 보임 |
| 2 | 첫 실행 제스처 안내 (V7) | 앱 데이터 초기화 후 첫 스트림 창 | 안내 오버레이 1회 표시, 확인/바깥 탭으로 닫힘, 창을 다시 열어도 미노출 |
| 3 | 저장 설정 유지 (V6) | 업데이트 전 FPS 배지를 켜둔 기기에서 업데이트 | 배지 여전히 켜짐. 신규 설치 기기에서는 배지·통계 HUD 모두 꺼짐 |
| 4 | 화면 기록 권한 배너 (신규) | 시스템 설정 → 개인정보 보호 및 보안 → 화면 기록에서 Leftcar Host 끄기 → 호스트 재실행 | 대시보드에 "화면 기록 권한 필요" 경고 배너와 "화면 기록 설정 열기" 버튼(클릭 시 해당 설정 창). 허용 → 재실행 → 배너 사라짐. 미허용 상태로 태블릿에서 스트림을 시작하면 뷰어에 화면 공유 권한 안내가 뜨고 호스트 배너는 하나만 유지 |
| 5 | 자연 스크롤 방향 (WIP 미검증분) | Mac 설정의 자연 스크롤 on/off 각각에서 태블릿 두 손가락 스크롤 | 두 경우 모두 Mac 트랙패드와 같은 방향으로 콘텐츠가 움직임 |

권한 미허용 상태 재현 대안: `tccutil reset ScreenCapture leftcar.ll3.kr` 후 호스트 재실행. 앱은 권한
프롬프트를 띄우지 않으므로(preflight 전용 설계) 배너의 버튼으로 시스템 설정에서 직접 허용한다.
