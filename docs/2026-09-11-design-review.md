# 디자인 리뷰 2026-09-11 — 애매함 제거와 적용 결과

뷰어 앱(Expo), 호스트 앱(Tauri) 전체 UX 조사 + 경쟁 앱 벤치마크(Chrome Remote Desktop,
Windows App, RustDesk, AnyDesk, TeamViewer, Parsec, Moonlight의 Android UX)를
통합한 리뷰. 조사는 코드 전수 탐색 2건(뷰어/호스트) + 웹 리서치 1건 병렬로 수행했다.

## 1. 수렴한 공통 문제 (두 앱 + 벤치마크가 같은 결론)

1. **용어 불일치** — 6자리 코드가 뷰어에서 "연결 번호/인증 번호/연결 코드" 3개,
   호스트에서 "연결 코드/인증 번호" 2개로 불렸다. 기기 명칭(호스트/컴퓨터/Mac/기기)도
   화면마다 달랐다. 사용자는 "Mac 화면의 그 번호"와 "앱 입력란"을 스스로 매핑해야 했다.
2. **핵심 동작의 무피드백** — 뷰어 호스트 행의 [연결] 탭은 최대 ~16초(3회 재시도)간
   아무 변화가 없었고, 전체화면 스트림 창엔 종료 경로가 시스템 제스처뿐이었다.
   벤치마크: Moonlight/CRD의 2탭 재연결, TeamViewer 툴바의 문서화된 X 종료가 표준.
3. **파괴적 동작 무확인** — 뷰어 "연결 기억 지우기"는 즉시 삭제 후 사후 알림,
   호스트 기기 삭제(revoke)는 확인 없이 즉시. 반대로 정기 동작인 공유 정지는
   모달로 확인받는 위험도 역전이 있었다.
4. **상태 비표시** — 오프라인(미발견) 최근 호스트가 발견된 호스트와 동일한 카드로
   렌더링됐다. 벤치마크 P0 패턴: CRD/Moonlight는 목록 자체에서 오프라인을 흐리게 표시.
5. **역할 혼란** — 뷰어의 페어링 화면 제목이 "연결 승인"(승인자는 Mac인데), 세션 토글은
   상태형 문구("원격 조작 허용됨"을 누르면 꺼짐)였다.

## 2. 적용한 수정 (이번 배치)

### 뷰어 앱

| # | 수정 | 파일 |
|---|------|------|
| V1 | 호스트 행 탭 시 해당 행 칩이 스피너 + "연결하는 중…"로 바뀜 | `apps/viewer-expo/app/host.tsx` |
| V2 | 401(승인 만료)로 진입하면 QR이 아닌 6자리 코드 입력이 기본 모드 — 안내 창 문구와 화면이 일치 | `apps/viewer-expo/app/pairing.tsx` |
| V3 | QR 스캔 8초 무반응 시 "Mac의 Leftcar에서 [연결 코드 만들기]를 눌러 QR을 표시해 주세요" 힌트(실패 지점 1회) | `pairing.tsx`, i18n `qrScanIdleHint` |
| V4 | 구버전 호스트 QR→코드 폴백 시 모드 전환 이유를 1회 안내(`pinFallbackNotice`) — 별도 `notice` 상태로 수명 관리 | `pairing.tsx` |
| V5 | "연결 기억 지우기" 실행 전 확인 다이얼로그(사후 알림 → 사전 확인) | `host.tsx` |
| V6 | 최근 호스트 중 지금 발견되지 않는(오프라인) 행의 연결 칩을 흐리게 — CRD/Moonlight 패턴 | `host.tsx` |
| V7 | 활성 스트림 카드에서 내부 세션 번호 `#N` 제거 | `app/catalog.tsx` |
| V8 | 전송 배지 폴백 "ADB" → "Wi-Fi"(소비자 칩에 개발자 은어 제거) | `src/transport-label.ts` + 테스트 |
| V9 | 매핑 안 된 오류의 영어 원문 괄호 표시 제거(원문은 콘솔로) | `src/control.ts` + 테스트 |
| V10 | 빈 탐색 상태에서 안내 카드가 뜰 때 설명 문구 중복 제거(안내는 실패 지점에서 한 번) | `host.tsx` |

### 호스트 앱

| # | 수정 | 파일 |
|---|------|------|
| H1 | 첫 화면 CTA 통일 — 헤더 버튼·모달·트레이·윈도우 제목이 모두 "연결 코드 만들기" | i18n `btnPair`/`pairingModalTitle`, `src-tauri/src/lib.rs` 트레이·윈도우 제목 |
| H2 | 대기 상태 3중 표현 정리 — 헤더 필 "대기 중", 카드 내 중복 상태 칩 삭제 | i18n `statusIdle`, `App.tsx` IdleStudioView |
| H3 | 세션 토글 상태형→동작형: "원격 조작 허용됨"(누르면 꺼짐) → "원격 조작 끄기" | i18n `remoteInputAllowed/Off` |
| H4 | 파일 공유 토글 "파일 전송: 켬/끔" + 감사 로그 전문 용어를 UI에서 제거(기록 자체는 유지) | i18n `fileShareToggle*/Hint` |
| H5 | 기기 삭제(revoke) 실행 전 확인 다이얼로그(단일/전체 모두) — 새 `RevokeConfirmDialog` | `PairingPanel.tsx`, `RevokeConfirmDialog.tsx` |
| H6 | "IP 복사" 하드코딩 한글 누수 → i18n `copyAddress` | `PairingPanel.tsx` |
| H7 | 종료 배너 "종료 이유:" 라벨 제거(상세가 이미 이유), 정지 모달 요약을 결과 중심 1줄로 | `App.tsx`, i18n `stopModalSummary` |
| H8 | 세션 카드 내부 번호 `#N`·가짜 만능 신호 막대 제거, 푸터 2초 하트비트 타임스탬프 제거 | `App.tsx` |
| H9 | 화면 기록 권한 배너에서 "다시 실행" 요구 제거 — 폴링이 곧바로 회복함을 반영 | i18n `screenPermBannerDesc` |

### 뷰어 네이티브(스트림 창)

| # | 수정 | 파일 |
|---|------|------|
| N1 | HUD에 "✕" 종료 칩 추가 — 시스템 바가 숨은 전체화면의 유일한 눈에 보이는 출구. `finish()` → onDestroy의 BYE 정리 경로 재사용 | `StreamHudController.kt`, `StreamActivity.kt` |
| N2 | 종료 토스트 5종 하드코딩 한국어 → `ViewerStrings.terminationMessage(reason)` (ko/en) | `ViewerStrings.kt`, `StreamActivity.kt` |

### 용어 통일 스윕 (두 앱 공통, `packages/ui-tokens/src/i18n.ts`)

- 6자리 코드: **연결 코드**(en: pairing code)로 통일 — `tabPin`, `pinTitle`, `pinDesc`,
  `invalidPinError`, `errPairingRejected`, `errPairingCodeInvalid`, 호스트 `pairingCodeLabel`.
- 뷰어 페어링 화면 제목 "연결 승인" → **"QR로 연결"**(승인 버튼은 Mac에만 존재),
  제출 버튼 "연결 승인하기" → "연결하기".
- "새 기기 연결하기"(뷰어, Mac 시점 문구) → "새 컴퓨터 연결하기". 홈 진입 버튼을
  목적지와 같은 계열로: "컴퓨터 찾기 →" → "컴퓨터 연결하기 →".
- 대기 배지 "연결 대기 중" → "연결 안 됨"(활동 없는 화면의 기대 표현 제거).
- 첫 화면·오류 문구에서 Tailscale 전문 용어 제거(문제 해결 가이드에는 유지).

## 3. 벤치마크에서 채택/기각

채택(이번 배치): 목록 내 오프라인 흐림(V6), 종료의 가시화(N1), 실패 지점 1회 안내(V3·V4),
파괴 동작 확인(V5·H5), 2탭 재연결 유지(기존 설계가 이미 Moonlight형 — 확인만).

기각/보류: 계정 시스템(Parsec/TeamViewer형) — LAN 무계정 정책 유지. 드래그 가능 플로팅
HUD(AnyDesk형) — 칩 4개로 충분, 옵션 확산 방지. 컨텍스트 메뉴형 툴바(TeamViewer형) —
현재 HUD 최소주의 원칙과 충돌. 제스처 문법은 이미 업계 표준(탭=클릭, 길게=우클릭,
두 손가락=스크롤, 핀치=로컬 줌)과 일치 — 변경 없음.

## 4. 보류(다음 배치 후보)

- 호스트 세션 카드의 화질 상한 슬라이더를 진단 패널 밖으로 이동(기능 이동이라 검증 필요)
- 파일 공유 카드 위치/계층 재배치(첫 화면에서 CTA 아래로)
- `streamTermination.ts`·진단 패널의 하드코딩 한국어 i18n화(제품 기본 언어가 한국어라
  영어 사용자에게만 드러남)
- 호스트 세션 카드에 페어링 기기 이름 매핑 표시(호스트가 addr만 제공 — Rust 확장 필요)
- 페어링 요청의 호스트 측 시스템 알림/트레이 배지(창이 닫혀 있으면 요청이 보이지 않는 구조)
- Windows 호스트의 kind-6 텍스트 입력·신규 문구 실측(Windows 빌드 체인 미검증)

## 5. 검증

- React Doctor: **100 / 100** (rules-of-hooks 위반 1건을 `use-stream-controller.ts`에서
  소스 수정으로 해결 — useMutation 호출을 컴포넌트 최상위로 펼침)
- 타입체크: 루트 `tsc -b`, viewer-expo, host-desktop 전부 통과
- vitest: 550 passed (용어 스윕 반영해 `transport-label.test.ts`, `control.test.ts`,
  `localized-error.test.ts` 갱신)
- Rust 호스트 크레이트 테스트: 131 passed
- Kotlin JVM 단위 테스트: 80 run, 0 failed
- 미검증: 실기기 스트림 창의 ✕ 칩 탭·토스트 문구(다음 실기기 검증 배치에서 확인),
  Windows 크로스컴파일
