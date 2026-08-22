# Leftcar Design System (DESIGN.md)

> **Identity**: High-Performance Desktop Streaming Utility (macOS / Windows / Android / Galaxy XR).
> **Philosophy**: **Human-Centric & De-AI Craftsmanship** · **Light Mode Default** · **System Sync** · **Intuitive Utility Tooling**.
> **Runtime & Package Manager**: `bun` (Fast native runtime & workspaces).
> **Benchmark Reference**: Apple AirDrop/Sidecar UI, Tailscale, Notion, Figma Desktop, Things 3.

---

## 1. De-AI & Human-Centric Design Philosophy

### 1.1 Why "De-AI"? (탈 AI 분석 및 체크리스트)
AI가 생성한 UI나 테크 데모 템플릿은 대개 다음과 같은 고질적인 문제점을 가집니다:
- **인위적인 올-다크(Obsidian) 강제**: 사용자의 OS 환경이나 낮 시간대 작업 조도와 무관하게 칠흑 같은 검은 화면 강요.
- **사이버펑크 네온 글로우 & 번쩍이는 애니메이션**: `box-shadow: 0 0 20px glow`, 레이더 링 파동, 인위적 펄스 효과로 눈의 피로도 유발.
- **불필요한 Glassmorphism (유리 효과) & 무지개 그라디언트**: 가독성을 떨어뜨리는 배경 블러와 의미 없는 보라-하늘색 그라디언트 남발.
- **엔지니어링 텔레메트리 덤프 (사용자 불친절)**: `AMediaCodec NDK`, `Zero-Copy Pipeline`, `Rustra JNI`, `cap-p95: 1.2ms` 같은 개발자 내부 용어를 첫 화면부터 나열하여 일반 사용자가 무엇을 해야 할지 알 수 없게 만듦.

### 1.2 The Leftcar Way (사람 친화적이고 특색 있는 요소)
Leftcar는 사용자가 백그라운드에 띄워두고 **"내 컴퓨터 화면을 폰이나 XR 헤드셋에서 빠르게 보고 조작하는"** 본질에 집중하는 실용 도구입니다:

| AI 템플릿 클리셰 (Don't) | Leftcar Human-Centric (Do) |
|---|---|
| 어두운 다크모드 고정 | **화이트(라이트) 모드 기본 + OS 시스템 테마 자동 연동** |
| 눈부신 네온 글로우 & 펄스 링 | **단정하고 명확한 1px 보더 + 자연스러운 소프트 섀도우** |
| 번쩍이는 인디고/퍼플 그라디언트 | **신뢰감 있는 솔리드 브랜드 블루 (`#2563EB`) & 명확한 시맨틱 컬러** |
| 복잡한 개발자 용어 도배 | **친근하고 명료한 상태 안내 ("스트리밍 준비 완료", "원격 조작 허용")** |
| 텔레메트리 텍스트 덤프 | **일반 사용자는 간결한 요약, 고급 측정치는 [세부 지표 보기] 토글로 격리** |
| 복잡한 메뉴 계층 | **원클릭 연결: [호스트 연결하기], [QR 페어링], [화면 열기]** |

---

## 2. Theme Architecture & Color Tokens

### 2.1 테마 정책 (Theme Policy)
1. **Light Mode Default**: 기본 테마는 눈이 편안하고 가독성이 뛰어난 **화이트/오프화이트 라이트 모드**입니다.
2. **System Auto Sync**: OS의 `prefers-color-scheme: dark` 설정을 감지하여 시스템이 다크 모드일 때 자동으로 유려한 다크 테마로 전환됩니다.
3. **Data Theme Override**: `[data-theme="light"]` 또는 `[data-theme="dark"]` 속성을 통해 수동 오버라이드가 가능합니다.

### 2.2 Design Tokens

#### Surfaces & Backgrounds
| Token | Light (Default) | Dark (System Sync) | Usage |
|---|---|---|---|
| `--bg-canvas` | `#F8FAFC` (Slate 50) | `#0B0F17` (Charcoal 950) | 최상위 앱/화면 캔버스 배경 |
| `--bg-surface` | `#FFFFFF` (Pure White) | `#111827` (Slate 900) | 카드, 패널, 모듈 기본 서피스 |
| `--bg-surface-subtle` | `#F1F5F9` (Slate 100) | `#1F2937` (Slate 800) | 보조 박스, 칩, 호버 배경 |
| `--bg-surface-active` | `#E2E8F0` (Slate 200) | `#374151` (Slate 700) | 활성 탭, 프레스 상태 |

#### Borders & Dividers
| Token | Light (Default) | Dark (System Sync) | Usage |
|---|---|---|---|
| `--border-subtle` | `#E2E8F0` (Slate 200) | `rgba(255, 255, 255, 0.08)` | 구분선, 서브 카드 테두리 |
| `--border-card` | `#CBD5E1` (Slate 300) | `rgba(255, 255, 255, 0.14)` | 메인 카드 윤곽선, 입력 필드 보더 |
| `--border-strong` | `#94A3B8` (Slate 400) | `rgba(255, 255, 255, 0.25)` | 포커스 및 모달 테두리 |

#### Typography & Text
| Token | Light (Default) | Dark (System Sync) | Usage |
|---|---|---|---|
| `--text-primary` | `#0F172A` (Slate 900) | `#F9FAFB` (Slate 50) | 제목, 주요 텍스트, 본문 |
| `--text-secondary` | `#475569` (Slate 600) | `#9CA3AF` (Slate 400) | 설명, 부제목, 라벨 |
| `--text-muted` | `#64748B` (Slate 500) | `#6B7280` (Slate 500) | 힌트 텍스트, 타임스탬프, 푸터 |
| `--text-dim` | `#94A3B8` (Slate 400) | `#4B5563` (Slate 600) | 비활성 아이콘, 비활성 캡션 |

#### Functional & Semantic Accents
| Role | Light (Default) | Dark (System Sync) | Purpose |
|---|---|---|---|
| **Brand Primary** | `#2563EB` (Blue 600) | `#3B82F6` (Blue 500) | 주 액션 버튼, 호스트 아이콘, 링크 |
| **Brand Hover** | `#1D4ED8` (Blue 700) | `#2563EB` (Blue 600) | 버튼 호버 상태 |
| **Success / Live** | `#059669` (Emerald 600) | `#10B981` (Emerald 500) | 정상 연결, 활성 스트림 세션 |
| **Success Subtle**| `#ECFDF5` (Emerald 50) | `rgba(16, 185, 129, 0.14)` | Live 배지 및 정상 칩 배경 |
| **Warning / Standby** | `#D97706` (Amber 600) | `#F59E0B` (Amber 500) | 대기 모드, 권한 요청 필요 |
| **Warning Subtle** | `#FFFBEB` (Amber 50) | `rgba(245, 158, 11, 0.14)` | Standby / 권한 배너 배경 |
| **Danger / Error** | `#DC2626` (Red 600) | `#EF4444` (Red 500) | 스트림 중단, 연결 끊김, 오류 |
| **Danger Subtle** | `#FEF2F2` (Red 50) | `rgba(239, 68, 68, 0.15)` | 에러 배너 배경 |

---

## 3. Screen & Component System (앱 & 데스크탑)

### 3.1 데스크탑 호스트 (Host Studio - Tauri/Web)
1. **Hero Status Card**: "스트리밍 준비 완료" / "현재 Galaxy XR로 1개 화면 전송 중" 등 직관적인 자연어 상태 표현과 [기기 페어링 QR] 액션 버튼.
2. **Session Cards**: 테이블 대신 읽기 쉬운 개별 모니터 카드 형태로 구성.
   - 디스플레이 명칭, 해상도, 대상 기기 IP, 재생률(FPS).
   - [원격 조작 허용됨 / 끄기] 직관적인 원터치 토글.
3. **Collapsible Inspector**: 캡처 레이턴시, 큐 대기 시간, 인코딩 시간 등 엔지니어링 텔레메트리는 기본적으로 숨겨져 있으며, `세부 지표 보기` 클릭 시 확장.
4. **Quick Info Strip**: 제어 포트, 입력 권한 상태, 비디오 엔진 요약을 하단에 단정하게 표시.

### 3.2 모바일 / XR 뷰어 (Viewer Hub - Expo/React Native)
1. **Hub / Home Screen**:
   - 상단: 내 기기 연결 상태 (연결됨 / 대기 중).
   - 메인: [화면 선택하기] 및 [호스트 연결하기] 대형 터치 타깃.
   - 간편 3단계 가이드: 컴퓨터에서 켜기 ➡️ 호스트 선택 ➡️ 공간에 배치.
2. **Nearby Host Picker**:
   - 에어드롭/블루투스 기기 목록처럼 깔끔하게 탐색된 Mac/PC 목록 표시.
   - 직관적인 수동 IP 입력 폼과 5GHz Wi-Fi 연결 팁.
3. **Screen Catalog & Quality Profiles**:
   - 모니터별 프리뷰 카드와 원터치 [XR 창 열기] 버튼.
   - 3단계 세그먼트 화질 프로필: `낮은 지연(1080p 60fps)` / `균형(1440p 60fps)` / `고화질(4K 60fps)`.
4. **Pairing Modal / Scanner**:
   - 단정한 카메라 스캔 프레임과 큼직한 6자리 코드 입력 필드.

---

## 4. De-AI Design Checklist for Future Updates

- [ ] **1. Light Mode First**: 기본 상태가 깔끔하고 시인성이 높은 화이트/라이트 모드로 설계되었는가?
- [ ] **2. System Sync**: OS가 다크모드일 때 자연스럽게 전환되며 텍스트 대비가 무너지지 않는가?
- [ ] **3. Human Microcopy**: 'Rustra JNI', 'Zero-Copy Pipeline' 대신 '화면 연결', '마우스 제어' 등 사용자 친화적 용어를 사용했는가?
- [ ] **4. No AI Glows**: 눈을 피로하게 만드는 네온 글로우(`0 0 16px glow`)나 펄스 리플 애니메이션을 배제했는가?
- [ ] **5. Progressive Disclosure**: 복잡한 디버그/엔지니어링 수치는 접이식 상세 보기로 격리하여 일반 사용자를 배려했는가?
- [ ] **6. Big Touch Targets**: 모바일/XR에서 손가락이나 핀치 제스처로 쉽게 누를 수 있도록 최소 44×44pt 터치 영역을 확보했는가?
