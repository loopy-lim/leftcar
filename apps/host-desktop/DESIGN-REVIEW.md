# host-desktop UI 디자인 감사 (DESIGN-REVIEW)

- 기준 문서: `site/design.md` §1 탈AI 원칙(G1~G6), §2 토큰, §3 컴포넌트 패턴 매트릭스, §4 카피 규칙
- 대상: `apps/host-desktop/index.html`, `apps/host-desktop/src/**` (감사 전용 — 구현 없음)
- 제약 준수: 세션·승인 게이트 등 기능 동작은 건드리지 않는 시각·위계 개선만 제안한다.
- 참고: design.md의 토큰 표는 site(랜딩) 기준이다. 앱은 `--font-sans`/`--font-mono`와 앱 자체 팔레트 토큰을 기준으로 삼되, "토큰 밖 임의 값 금지"와 금지 패턴 G1~G6은 동일하게 적용해 판정했다.

---

## 0. 총평

앱의 뼈대는 이미 탈AI 원칙과 잘 맞는다. 헤더–본문–푸터 상태바 구조, Idle 화면의 단일 카드+CTA, 설정 모달의 "정의형 행(행 제목+설명+스위치)" 패턴, 인스펙터가 토글로 접힌다는 점은 design.md §3의 그릇 매트릭스와 같은 방향이다. 그림자·radius·모노스페이스 숫자 등 크로마도 절제되어 있다.

문제는 (1) **진단 UI와 사용자 UI의 경계가 절반만 지켜진 것**(품질 슬라이더·실험 노브가 개발자 영역에 묻힘/사용자 영역에 노출), (2) **토큰 밖 임의 값의 산재**(Tailwind 유틸리티 색, TSX 인라인 hex, index.html 부팅 화면 별도 팔레트), (3) **pill/박스 크롬의 재축적**(G1·G6 유형), (4) **같은 상태 정보의 4중 표기**다. 아래 이슈 목록과 우선순위 요약(§6)을 따른다.

---

## 1. 축별 상세 — 이슈 목록

### 축 ① AI-티 패턴 (design.md 금지 목록 대조)

#### A-1. pill/박스 크롬에 상태 사실을 담는 습관 — G1 위반 (재발)
- **근거**
  - `src/index.css:680-686` `.input-request-badge` — `border-radius: 999px` + accent 배경 + 펄스 애니메이션 배지
  - `src/index.css:1833-1841` `.footer-tag-pill` — `border-radius: 999px` pill에 "클립보드 공유"/"커튼" 라벨
  - `src/index.css:277-290` `.host-status-pill` — `border-radius: 20px` pill(사실상 999px와 같은 그릇)
  - `src/index.css:1638-1644` `.count-pill`, `src/index.css:570-579` `.live-badge-pulse`, `src/index.css:1587-1594` `.countdown-badge`, `src/index.css:651-660` `.session-tag`, `src/index.css:259-267` `.host-version-badge`
- **영향**: design.md G1 판정법("pill 없이 읽어도 자연스러운가")에 걸린다. "버전 0.1.4", "연결됨", "3대 연결 중" 같은 사실은 텍스트 한 줄이면 충분하고, pill이 8종류나 반복되면 크롬이 정보보다 앞선다. 헤더 pill + 섹션 LIVE 배지 + 카드 푸터 상태로 동일 정보가 중복 표기되는 것도 여기서 온다(§2 D-1과 결합).
- **수정안**
  1. `.host-version-badge`, `.session-tag`, `.count-pill`은 pill을 걷고 `--font-mono` + `--text-muted` 텍스트로 강등한다(예: `0.1.4`, `#3`, `기기 2`). design.md §1 G1 대안과 동일한 수법.
  2. `.footer-tag-pill` 2종은 푸터에 "클립보드 공유 · 커튼" 인라인 텍스트 하나로 합친다(활성 기능이 무엇인지 나열이면 충분).
  3. `.input-request-badge`만 예외로 둔다 — 행동 요청(alarm)이며 펄스가 필요한 유일한 배지다. 단 radius는 카드 시스템에 맞춘다.
  4. `.host-status-pill`은 dot+텍스트로 유지하되 radius를 6~8px로 낮춰 "pill" 인상을 제거한다.
- **난이도**: 소 / **임팩트**: 중

#### A-2. 세션 카드의 4칸 균일 메트릭 그리드 — G2 위반
- **근거**
  - `src/index.css:697-731` `.stream-card-metrics-grid` (`repeat(4, 1fr)`) + `.metric-card` — 표면·크기·형태가 같은 카드 4개
  - `src/components/SessionCard.tsx:106-127` 인코더 FPS / 비트레이트 / 연결 상태 / 전송 안정성을 같은 그릇에 나열
- **영향**: 서로 다른 질문(성능/상태/품질)을 같은 그릇에 담는 design.md G2 패턴 그대로다. 특히 "연결 상태: 실행 중" 칩은 헤더 pill과 카드 푸터에서 이미 말한 정보의 3번째 반복이며, "안정성: 안정"은 정상일 때 정보량이 0이다.
- **수정안**: 4칸 그리드를 **정의형 행 1줄로 붕괴**시킨다 — `22.4 FPS · 8.2 Mbps · 안정` (mono, `--text-secondary`) 한 줄 + 이상 감지 시에만 rose 텍스트(`프레임 3 끊김`)가 뜨는 구조. 카드 4개의 박스 크롬을 없애면 카드 본체가 세션 식별 + 액션 + 상태 한 줄로 정리된다. 수치 상세는 인스펙터(이미 존재)가 담당한다.
- **난이도**: 중 / **임팩트**: 대

#### A-3. 문제 해결 모달의 균일 카드 5개 — G2/G6 경계
- **근거**: `src/modals/DashboardModals.tsx:282-316` `troubleshoot-card` 5개가 아이콘+제목+설명 동일 구조로 반복. `src/index.css:1485-1489` 카드 크롬(표면+보더+radius).
- **영향**: 읽을 거리(순차 가이드)를 박스 나열로 처리 — "박스에 넣기" 해법의 반복(G6). 항목이 5개면 박스 없는 리스트 행(구분선만)이 읽기 쉽다.
- **수정안**: 박스 크롬을 제거하고 `settings-item-row`와 같은 **행 + 하단 1px 구분선** 리스트로 교체한다(컴포넌트 재사용으로 CSS 추가 0).
- **난이도**: 소 / **임팩트**: 소

#### A-4. (양호) 확인 완료
- 빈 상자 모식도(G3) 없음 — 모든 도식성 요소는 실데이터(QR, 카운트다운, 시그널 바)를 가진다.
- 무근료 중앙정렬(G5) 없음 — 중앙정렬은 Idle 카드와 페어링 QR 등 "그릇이 중앙인 곳"에만 쓰였다.
- hover 피드백은 인터랙티브 요소에만 있다.

### 축 ② 대시보드 구조 (정보 위계, 카드 남발, 섹션 구분)

#### D-1. 스트리밍 상태 정보의 4중 표기
- **근거**
  1. `src/components/DashboardHeader.tsx:56-63` 헤더 상태 pill "N대 연결 중"
  2. `src/components/StreamsListView.tsx:46-49` 섹션 제목 옆 `live-badge-pulse` "LIVE"
  3. `src/components/SessionCard.tsx:119` 메트릭 카드 "연결 상태: 실행 중"
  4. `src/components/SessionCard.tsx:148` 카드 푸터 우측 `sessionStateLabel(..., badge=true)` 다시 "실행 중"
- **영향**: 같은 사실이 한 화면에 4번 렌더링된다. 위계가 아니라 반복이고, 이것이 A-1의 pill 남발과 맞물려 "상태 배지 컬렉션" 인상을 만든다.
- **수정안**: 표기 원칙을 "헤더 1회(전역), 카드당 1회(개별)"로 한정한다. (2) LIVE 배지는 섹션 제목으로 흡수(`활성 스트림 2`가 이미 의미를 가짐 — 삭제), (3)은 A-2 수정안의 상태 한 줄에 통합, (4) 카드 푸터는 자동 정리 정책 안내만 남긴다.
- **난이도**: 소 / **임팩트**: 중

#### D-2. 푸터 상태바와 헤더의 역할 중복
- **근거**: `src/components/DashboardFooter.tsx:50-76` 주소/원격 제어 권한 칩 + 활성 기능 pill + 설정 버튼 / `DashboardHeader.tsx:65-98` 우측에도 설정·도움말 버튼.
- **영향**: 설정 진입점이 헤더 아이콘과 푸터 텍스트 버튼 2곳. 푸터 우측의 "설정"은 중복이며 좁은 38px 바에서 공간을 낭비한다.
- **수정안**: 푸터 설정 버튼을 제거하고 `footer-timestamp` 자리의 플랫폼 라벨만 남긴다. 설정은 헤더 단일 진입(⌘, 있음)으로 확정.
- **난이도**: 소 / **임팩트**: 소

#### D-3. (양호) Idle 화면
- `src/components/IdleStudioView.tsx` + `index.css:529-570` — 단일 카드, 아이콘 1개, 제목+설명+CTA 1개+단축키 힌트. 카드 남발 없이 이상적인 휴면 위계. 유지한다.
- 단, CTA 안 `.kbd-shortcut`의 인라인 반투명 화이트 오버라이드(`IdleStudioView.tsx:34-39`)는 `btn-lg` 흑백 반전에 의존하는 임시방편이다. `.btn-lg .kbd-shortcut` 규칙으로 CSS로 옮기면 토큰 준수가 된다(축 ③).

### 축 ③ 토큰 준수 (임의 크기/색상/px)

#### T-1. Tailwind 유틸리티 색상이 팔레트 토큰을 우회 — 최우선 토큰 위반
- **근거**: `src/diagnosticStyles.ts:7-8` — `!text-sky-600`, `!text-amber-600` (Tailwind 기본 팔레트의 sky-600 `#0284c7`, amber-600 `#d97706`).
- **영향**: 앱 팔레트는 blue(`#3b82f6`/`#2563eb`)와 amber(`#f59e0b`/`#d97706`)인데 인스펙터 경고 톤만 Tailwind 별개 색을 쓴다. 다크 테마에서 `amber-600`은 `--accent-amber-subtle` 배경 위 배지류의 `#f59e0b`와 미묘하게 다른 주황으로 렌더링되어 같은 "경고"가 두 색으로 보인다. `!important` 접두사는 토큰 시스템 위의 예외 통로가 된다.
- **수정안**: `diagnosticValueVariants`의 tone을 CSS 변수로 교체 — `active: "inspector-tone-active"`, `warning: "inspector-tone-warning"`을 `index.css`에 `color: var(--accent-blue)`/`var(--accent-amber)`로 정의. Tailwind 유틸리티 의존 제거.
- **난이도**: 소 / **임팩트**: 중

#### T-2. TSX 인라인 하드코딩 색상·스택
- **근거**
  - `src/Indicator.tsx:81,98` — `#fafafa`, `#f43f5e`(팔레트에 없는 rose) + 자체 font-family 스택(`index.css`의 `--font-sans`와 "Apple SD Gothic Neo"만 다름)
  - `src/Privacy.tsx:277,283` — 커튼 힌트 `#666`
  - `src/PairingPanel.tsx:244-245` — QR 색 `#09090b`/`#ffffff`(값은 우연히 일치하나 리터럴)
  - `src/modals/DashboardModals.tsx` 16곳, `src/PairingPanel.tsx` 8곳 등 총 50+ 인라인 `style={{}}`(px 폰트/간격 반복)
- **영향**: 인디케이터는 독립 창이지만 토큰이 아니면 테마 변경·브랜드 색 조정 시 누락된다. `#f43f5e`는 rose-500으로, 앱의 `--accent-rose`(#ef4444/#dc2626)와 또 다른 세 번째 빨강이다. 인라인 스타일은 G6적 템플릿 흔적이자 토큰 회로망 밖 값이다.
- **수정안**: ① Indicator/Curtain/QR 색을 토큰 참조로 교체(인디케이터는 항상 다크이므로 다크 전용 토큰 1~2개 추가는 허용 범위). ② 반복 인라인 스타일은 `.pairing-advanced-toggle`, `.card-footer-meta` 등 클래스로 승격. ③ "인라인 style 신규 금지"를 앱 규칙으로 기록.
- **난이도**: 중 / **임팩트**: 중

#### T-3. index.html 부팅 화면이 별개 팔레트 — 플래시 불일치
- **근거**: `index.html:5-10,16-33` — `#f8fafc`/`#0b0f17`/`#111827`/`#64748b`/`#0f172a`(slate). 앱 토큰은 `#fafafa`/`#09090b`/`#141417`/`#71717a` 계열.
- **영향**: 앱 시작 시 부팅 카드 → React 마운트 순간 배경색이 slate→zinc로 미세하게 튄다. 같은 "다크"가 두 종류다.
- **수정안**: 부팅 화면 색을 앱 라이트/다크 토큰 값과 동일한 리터럴로 교체(`#fafafa`, `#09090b`, 표면 `#141417`, 텍스트 dim `#71717a`). `theme-color` 메타도 동일 값.
- **난이도**: 소 / **임팩트**: 소

#### T-4. 스위치 다크 오버라이드의 하드코딩 + 중복 토큰
- **근거**
  - `src/index.css:2197-2205` `[data-theme="dark"] .ui-switch:not(.switch-active)` — `#27272a`/`#3f3f46`/`#a1a1aa` 리터럴(토큰 값과 동일한데 변수를 안 씀)
  - `src/index.css:16-24 vs 76-84 vs 133-141` — `--accent-primary`와 `--accent-blue`가 라이트/다크 모두 완전 동일 값으로 2벌 정의
- **수정안**: 스위치 오버라이드를 `var(--bg-surface-hover)`/`var(--border-strong)`/`var(--text-secondary)`로 치환하고, `--accent-primary`를 남기고 `--accent-blue`군을 제거(사용처 전수 치환)해 토큰 수를 줄인다.
- **난이도**: 소 / **임팩트**: 소

### 축 ④ 진단·개발자용 UI와 사용자용 UI의 혼재

#### X-1. 품질 슬라이더(사용자 제어)가 개발자 인스펙터 안에 갇혀 있음
- **근거**: `src/SessionEncoderDiagnostics.tsx:29-53` `QualityOverride`(수동 화질 상한 슬라이더+자동 복귀)가 `SessionEncoderDiagnostics` 내부. 이 인스펙터는 `src/components/StreamsListView.tsx:47-54`의 `showMetrics` 토글(기본 꺼짐) 뒤에 있다. `src/components/SessionCard.tsx:97` `showInspector &&` 게이트.
- **영향**: "화질 낮추기"는 화면 공유 사용자의 합리적 행동인데, 기본 화면에서는 존재조차 보이지 않는다. 반대로 인스펙터를 연 개발자 화면에는 사용자 제어가 섞여 패널 성격이 흐려진다. 진단 UI가 기본 화면을 어지럪히는 문제는 없으나(토글 잘 됨), **사용자 기능이 진단 UI에 갇힌 역방향 혼재**가 있다.
- **수정안**: `QualityOverride`를 인스펙터에서 분리해 세션 카드 본체(상태 한 줄 아래, 접힘 없는 2차 행)로 옮긴다. 인스펙터는 읽기 전용 텔레메트리만 남긴다. 로직(props)은 그대로 두고 렌더 위치만 이동 — 승인 게이트 등 기능 불변.
- **난이도**: 중 / **임팩트**: 대

#### X-2. 실험 노브(페이싱 A/B)가 설정 모달에 무경계 노출
- **근거**: `src/ExperimentsSection.tsx:157-216` — `sndbufBytes`(65536~2097152), `drlWindowMs`, `pacingBudgetPct` 등 전송 계층 튜닝 값이 설정 모달 일반 섹션에 평등하게 나열. 제목만 "실험"이고 경고·분리 없음. `useExperiments`는 `src/Privacy.tsx:186-254`.
- **영향**: 개발자 A/B 노브와 클립보드 공유 같은 사용자 프라이버시 설정이 같은 그릇(정의형 행 리스트)에 섞인다. 잘못 만지면 스트림 품질이 나빠지는 값을 사용자가 "설정"이라는 신뢰 아래 만지게 된다.
- **수정안**: ExperimentsSection을 "고급" 접기(details/토글)로 한 단계 내리고, 섹션 상단에 되돌리기(기본값 복원) 버튼을 둔다. 숫자 5개+스위치를 전부 펼쳐 보여주는 대신 기본은 접힌 상태.
- **난이도**: 소 / **임팩트**: 중

#### X-3. 인스펙터 텔레메트리의 무위계 플랫 그리드 (개발자용이지만 가독성 문제)
- **근거**
  - `src/SessionPipelineDiagnostics.tsx:143-156` — Rates→Timing→Transport→Queue→Fec→Recovery→Tail→Target 순서로 ~25개 Metric이 연속 렌더
  - `src/SessionInspector.tsx:47-50` `.inspector-grid` (`repeat(auto-fit, minmax(130px, 1fr))`) — 모든 항목이 같은 크기 셀에 배치, 래깅 발생
  - `src/index.css:807-812` 그리드 간격만 있고 그룹 구분 시각장치는 `inspector-divider` 1개
- **영향**: 인스펙터는 토글 뒤에 있어 기본 화면 오염은 없다(양호). 그러나 열어보면 25개 수치가 위계 없는 벽이며, "경고 tone"만 색으로 구분된다. 개발자 UI라도 값의 중요도(실패 신호 vs 참조값)가 안 보인다.
- **수정안**: ① `auto-fit`을 고정 2~3열로 바꿔 래깅 제거, ② 파이프라인 스테이지별 소제목(`inspector-header` 재사용)을 그룹 사이에 삽입 — 이미 존재하는 2그룹 구조를 PipelineDiagnostics 내부 5~6그룹으로 확장, ③ warning tone 항목을 상단 "주의" 행으로 모아 스캔 시간을 줄인다.
- **난이도**: 중 / **임팩트**: 중

### 축 ⑤ 다크 테마 일관성

#### C-1. (대체로 양호) 테마 3모드 시스템은 일관됨
- `src/App.tsx:110-122` `data-theme` 속성 + `prefers-color-scheme` 이중 경로가 `index.css:64-160`에서 같은 값 세트로 수렴 — 구조는 올바르다.
- 예외는 T-3(부팅 화면), T-4(스위치 리터럴), T-1(tone 색) 3건이며, 수정하면 다크 일관성은 사실상 완성이다.
- 참고 1: `index.css:1381-1388` `.pairing-status-dot`의 `rgba(16,185,129,0.4)` 글로우도 리터럴 — `--accent-emerald-subtle` 참조로 치환 대상(난이도 소).
- 참고 2: `src/index.css:1710-1716` `.qr-image-frame`은 항상 흰 배경이나 QR 스캔 신뢰성 목적의 의도된 예외로 유지한다.

#### C-2. 앱 팔레트와 site 팔레트의 브랜드 단절 (기록용 — 이번 라운드 결정 사항 아님)
- **근거**: site는 `--accent: #4cc2a8`(청록, `site/design.md` §2.3), 앱은 Tailwind blue/emerald(`index.css:16-45`). 사이트에서 "다운로드한 앱"이 전혀 다른 파랑을 쓴다.
- **영향**: 브랜드 연속성 상실. 단, 팔레트 근거는 design.md §5의 열린 결정(P3)으로 site 워커의 몫이므로 앱 단독 선수교체는 하지 않는다. **site 팔레트 결정이 나면 앱 `--accent-*` 4군을 같은 값으로 동기화하는 후속 작업만 정의해 둔다.**
- **난이도**: 소(결정 후) / **임팩트**: 중

---

## 2. 우선순위 요약 (상위 5)

| 순위 | 이슈 | 요약 | 난이도 | 임팩트 |
|---|---|---|---|---|
| **1** | X-1 | 화질 슬라이더가 개발자 인스펙터에 갇힘 — 카드 본체로 승격 (사용자/진단 UI 역방향 혼재 해소) | 중 | 대 |
| **2** | A-2 + D-1 | 세션 카드 4칸 균일 메트릭 그리드(G2)를 정의형 한 줄로 붕괴 + 상태 4중 표기를 헤더 1회·카드 1회로 정리 | 중 | 대 |
| **3** | T-1 (+T-4, C-1 참고) | 인스펙터 tone의 Tailwind `!text-sky-600`/`!text-amber-600`을 앱 팔레트 토큰으로 교체 — 다크에서 경고색 2종 문제 해소 | 소 | 중 |
| **4** | A-1 | pill 8종(version/session-tag/count/LIVE/countdown/footer-tag)을 텍스트 강등 — design.md G1 대안 적용 (행동 배지 1종만 예외) | 소 | 중 |
| **5** | X-2 + X-3 | 실험 노브를 설정 모달의 접힌 "고급" 섹션으로 강등 + 인스펙터 그리드 고정열·그룹 소제목화 | 중 | 중 |

후속(비우선): T-2(인라인 스타일/하드코딩 색 정리), T-3(부팅 화면 팔레트 정합), T-4(스위치 리터럴·중복 토큰 제거), D-2(푸터 설정 버튼 중복), A-3(문제 해결 모달 행 리스트화), C-2(site 팔레트 결정 후 앱 동기화).

---

## 3. 실행 시 주의 (구현 워커용)

- 모든 수정은 시각·위계에 한정한다. `useHostStatus`/`useSessionActions`/`toggleGate`/승인 명령(`approve_pending_pairing` 등)의 호출 그래프는 변경하지 않는다.
- X-1 이동 시 `QualityOverride`의 props(onSetQuality, qualityBusy, disabled 조건)를 그대로 유지해 동작 등가를 보장한다.
- A-2/D-1 축소로 인한 번역 키 감소(`statusRunning`, `liveBadge` 등)가 생기면 `@leftcar/ui-tokens` 스키마와 viewer-expo 공용 번역 영향을 확인한다.
- 구현 후 게이트: 저장소 루트에서 `npx -y react-doctor@latest . --verbose` 100/100 + typecheck + `apps/host-desktop` 관련 테스트(hostState, Privacy, streamTermination, encoderDiagnostics).
