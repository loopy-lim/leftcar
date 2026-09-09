# XR 대응 미흡 체감 원인 조사 — 2026-09-09

## 0. 요약

사용자가 "XR 대응이 미흡하다"고 느끼는 체감의 원인을 코드·실측 문서·플랫폼 기준점 세 면에서 조사했다. 결론:

- **XR 고유 코드는 "창 비율 프리셋 4개" 한 경로뿐이다.** `StreamActivity.kt:82-120`의 `hasSystemFeature("android.software.xr.api.spatial")` → `Session.create` → `setPreferredAspectRatio`가 전부이고, 그마저도 Galaxy XR 실기 검증이 전부 미수행(`viewer-display-sizing-validation.md:61-65` 빈 체크박스, `EVIDENCE.md:15` 태블릿 대체 검증).
- **메인 UI는 manifest 차원으로 세로 폰이다.** 세로 고정(`AndroidManifest.xml:24`), 9~13px 폰트, 4~8px 패딩 터치 타깃, 전 화면 1열 세로 스크롤, 뷰포트/브레이크포인트 로직 0건. Google 기준으로는 명시적으로 "less optimal(모바일 호환 티어)" 분류에 해당한다.
- **입력이 "원격 데스크톱 터치 매핑"이지 XR 입력이 아니다.** XR 컨트롤러·gaze+pinch·핸드 트래킹 수신 코드 0건. 실츬 입력 지연 130~160ms. 텍스트(IME) 입력 경로 자체가 없다.
- 상당 부분은 PRD P-04 "XR 최소 사용"(`01-product-requirements.md:49-51`)이라는 **의도된 결정의 결과**다. 즉 미흡함은 버그가 아니라 범위 선택의 누적 체감이다.

## 1. 조사 방법

- 탐색 에이전트 3개: (a) viewer XR 감지/분기·UI 전수, (b) 입력·세션 UX 전수, (c) 외부 기준점(Android XR 품질 가이드, Galaxy XR 스펙, VR 데스크톱 스트리밍 앱 관습). 문서 조사는 직접 수행.
- 근거 표기: `file:line`. "없음 확인"은 저장소 전수 grep 기반. "추측"은 코드 부재로부터의 필연적 귀결이지만 실기 미검증.

## 2. 사용자 체감별 finding

### A. "XR용으로 만들어진 게 하나도 없는 것 같다" — XR 분기가 사실상 1개

- XR 감지는 `StreamActivity.kt:82` 피처 프로브 1곳 + `StreamLauncherModule.kt:352-361`의 동일 프로브(비율 행 숨김용)뿐. 통과 시 하는 일은 `SpatialWindow.setPreferredAspectRatio`(16:10/16:9/4:3/9:16) 전부다(`StreamActivity.kt:96-120`, `SpatialWindowBridge.java:12`).
- 스트림 창은 manifest `XR_ACTIVITY_START_MODE_HOME_SPACE`(`AndroidManifest.xml:47`)로 항상 홈 스페이스 2D 패널. Full-space, `SpatialCapabilities` 리스너, `androidx.xr.compose`/SpatialPanel/Entity 참조 0건.
- 창 이동·크기·거리는 전부 Android XR 시스템 기본 동작 위임(PRD FR-007 의도). 앱이 제공하는 공간 경험은 0개.
- 유일한 XR 기능(비율 프리셋)조차 실기 미검증 — 비율 전환·복원·9:16 세로 창 bounds 충돌 체크가 전부 빈 상태(`viewer-display-sizing-validation.md:61-65`).
- manifest에 `uses-feature android.software.xr.api.spatial` 선언이 없다(런타임 프로브만). 설치 시점에 XR 앱으로 분류되지 않을 수 있다(추측).

### B. "먼 곳에서 세로 폰 앱을 보는 것 같다" — 폰 기준 UI 잔존

- 세로 고정: `AndroidManifest.xml:24` `screenOrientation="portrait"`. 넓은 패널에서도 세로 폰 1열 레이아웃. `useWindowDimensions`/브레이크포인트/멀티컬럼 로직 0건.
- 초소형 폰트: 품질 탭 상세 9px(`catalog-styles.ts:198`), 칩·배지·포트 10px(`:338,:400,:441`), 버튼 다수 11px, 카드 본문 12~13px, 헤더 타이틀 15px(`_layout.tsx:34`). XR 가이드 최소 폰트는 **14dp(1.75m 기준)**.
- 작은 터치 타깃: 액션 버튼 paddingVertical 4(`catalog-styles.ts:85-94`), 칩 6/1(`:329-336`), hitSlop 6/8(`host.tsx:178,216`). XR 가이드 최소 **48×48dp**.
- 폰 시스템 UI 가정: `expo-status-bar`(`_layout.tsx:25`), `SafeAreaView` edges(`index.tsx:230`) — XR 패널에는 없는 개념.
- 스트림 HUD도 폰 스케일: 오버레이 텍스트 10~15f, 최소 터치 20dp(`StreamHudController.kt:284-294,427`).
- Google 품질 가이드는 이 상태를 명시적으로 "모바일 티어 = 덜 최적화된 경험"으로 정의한다(https://developer.android.com/docs/quality-guidelines/android-xr).

### C. "조작이 XR답지 못하다" — 입력 모델이 터치 매핑

- 입력 처리는 `SOURCE_TOUCHSCREEN/MOUSE/STYLUS`만(`StreamActivity.kt:487-544`). gaze+pinch·핸드 트래킹·XR 컨트롤러 트랙패드 수신 코드 **없음 확인**(androidx.xr 입력 API 참조 0건).
- XR 컨트롤러는 Android가 라우팅하는 일반 마우스 이벤트로만 동작. 두 손가락 스크롤·long-press 우클릭은 `SOURCE_TOUCHSCREEN` 제스처라 컨트롤러에서는 발화하지 않을 가능성이 높다(추측 — 실기 확인 필요).
- 입력 지연 실츬 130~160ms, 유실 시 수 초 정지(`tablet-input-response-validation.md:20-37`). 지연 보정/예측 코드 없음(없음 확인).
- 좌표는 정규화 절대 매핑 + `coerceIn(0,1)`(`StreamWindowGeometry.kt:52-70`)이라 레터박스 탭이 화면 가장자리 클릭이 된다. 감도/가속 조절 없음.
- 스크롤 속도는 뷰 픽셀 고정(`DEFAULT_LINES_PER_PIXEL = 1/25f`, `StreamTouchGestures.kt:44-48`)이라 큰 패널에서 체감 속도가 어긋난다.
- 커서는 18×22dp 고정 흰 화살표 오버레이(`CursorOverlayView.kt:154-175`)로 실제 Mac 커서 모양 미반영, 4K 분할 모드에선 꺼져 비디오에 박제된 커서가 지연만큼 늦게 보인다(`CaptureSession+Cursor.swift:21-27`).

### D. "타이핑이 안 된다" — 텍스트 입력 경로 부재 (최대 기능 공백)

- 입력 프로토콜에 텍스트 이벤트가 없다(`input_protocol.rs` — keyCode/scanCode만). `commitText`/`InputConnection` 처리 0건 → XR 소프트키보드·컨트롤러 텍스트 입력으로 Mac에 타이핑 불가. 이미 공식 백로그(`2026-09-08-usability-review.md` §4).
- 키는 Android keyCode → macOS 가상 키코드 하드 매핑(`CaptureSession+Input.swift:230-253`)이라 IME 조합 한글은 도달 불가, 미매핑 키는 조용히 폐기(`:257`).

### E. "화면을 크게/편하게 못 쓴다" — 경쟁 앱 표준 패턴 부재

- VR 데스크톱 앱(Virtual Desktop, Meta Virtual Display, Google PC Connect)의 표준: 초대형 가상 스크린 + 크기/거리 슬라이더, 곡면 조절, 팔로우/헤드락, 리센터, 극장 모드(풀스페이스+패스스루 디밍), 컨트롤러 툴바. Leftcar엔 이 중 어떤 것도 없음(없음 확인).
- Galaxy XR 홈스페이스 패널 최대 2,560×1,800dp가 물리 한계 — "크게 보기"의 정석 경로는 풀스페이스 전환 + 레터박스 제거인데 앱에 진입점이 없다.
- 콘텐츠 위 오버레이 HUD는 XR 재생 컨트롤 가이드의 명시적 안티패턴. 어두운 곳에서 손 트래킹이 실패하는 Galaxy XR 환경 특성상 고정 UI/orbiter 필요성이 더 크다.

### F. "가끔 버벅고 끊긴다" — 품질 체감 (XR 특화 처리 부재 포함)

- 4K 분할(2디스플레이) 렉: 호스트 랑데부 83ms 만료 → 전체 리셋+페어 IDR 폭풍, 실츬 2.9초 정지. 완화 실험 계획까지 문서화돼 있고 미수행(`2026-09-08-two-display-lag-research.md`).
- 60fps 소스를 90Hz 패널에 보여줄 때의 저지터 처리, foveation 등 XR 렌더링 특유 처리 **없음 확인**.
- reconfigure(해상도/품질 변경) 시 창이 "다시 연결할 준비 중"으로 한 번 끊기고, 적응형 해상도가 사용자 요청 없이 자동 재바인드를 건다(`use-stream-controller.ts:319-373`).

### G. 이미 잘 된 것 (체감 원인 아님)

QR 승인 페어링(fb188f3), 무음 자동 재접속, 같은 창 복구, 입력 잠금 배너, 제스처 안내 재열람, 오디오 토글(4dd192b) — 세션 라이프사이클 UX는 최근 개선돼 있다. "미흡" 체감은 세션 연결이 아니라 **XR 네이티브 레이어 자체**에서 온다.

## 3. Galaxy XR 실기 확인 체크리스트 (사용자 수행 권장)

1. 홈/카탈로그를 패널에서 열어 글자 크기·터치 타깃 체감 확인 (9~13px 폰트, 48dp 미만 타깃).
2. 컨트롤러로 스트림 조작: 탭/드래그는 되는지, 두 손가락 스크롤·long-press 우클릭이 발화하는지(§C 추측 검증).
3. 소프트키보드를 열어 타이핑 → Mac 미반영 확인(§D).
4. 창을 최대로 확대 + 비율 프리셋 4개 전환·복원 — 유일한 XR 기능이자 미검증 영역(§A).
5. 1.75m 거리에서 스트림 속 텍스트 가독성 (14dp 기준 대비).
6. 어두운 방에서 손 트래킹 상태에서 앱 조작 가능 여부(고정 UI 부재 영향).

## 4. 개선 옵션 (우선순위 제안, 미수행)

| 옵션 | 해소하는 체감 | 규모 |
| --- | --- | --- |
| 텍스트 입력 프로토콜(IME commitText → Mac 주입) | §D 타이핑 불가 | 프로토콜 신설 + 호스트 주입 (백로그 명시) |
| XR 입력 수신(컨트롤러 제스처 → 스크롤/우클릭 매핑) | §C 조작감 | 뷰어 네이티브 |
| UI 밀도 상향(폰트/타깃 XR 스케일, 가로 레이아웃 분기) | §B 폰 앱 체감 | RN 스타일·레이아웃 |
| 풀스페이스 극장 모드(패스스루 디밍 + 레터박스 제거) | §E 화면 크기 | androidx.xr.compose 도입 |
| 2디스플레이 랑데부 완화(연구 문서 실험 1) | §F 버벅임 | 호스트 캡처 shim |
| 비율 프리셋 Galaxy XR 실기 검증 | §A 미검증 | 측정만 |
