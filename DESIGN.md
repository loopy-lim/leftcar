# Leftcar Design System (DESIGN.md)

> **Identity**: Ultra-Simple Desktop Streaming & Multi-Display Utility (macOS / Windows / Android).
> **Philosophy**: **Ultra-Simple · Human-Centric & De-AI Craftsmanship · Dark & White Monochrome Tones · System Sync · Zero Distraction**.
> **Runtime & Package Manager**: `bun` (Fast native runtime & workspaces).
> **Benchmark Reference**: Apple AirDrop/Sidecar UI, Tailscale, Linear (Minimalist view), Things 3, Raycast.

---

## 1. Ultra-Simple & Monochrome Philosophy

### 1.1 Why "Simple is Best & Dark/White"?
- **불필요한 장식 요소 및 색상 배제**: 무지개 그라디언트, 네온 글로우, 인위적인 뱃지, 불필요한 색상 과용을 완전히 제거합니다.
- **Dark & White (모노크롬 / 흑백 / 그레이스케일) 미학**: 순수한 흑백과 징크(Zinc/Slate) 계열의 단정한 중간 톤을 사용하여 눈의 피로를 최소화하고 콘텐츠(화면, 수치) 자체에 몰입하게 합니다.
- **Tabular Numbers (`tnum`) 기본 적용**: FPS, 지연 시간, 대역폭, 포트 등 실시간으로 변하는 숫자의 너비를 고정하여 레이아웃 떨림과 시각적 혼란을 없앱니다.
- **직관적인 와이어프레임 & 미니어처**: 복잡한 설명 대신 16:9, 16:10, 21:9 등 화면비에 맞춘 정갈한 라인아트 미니어처로 즉각적인 인지성을 제공합니다.
- **정돈된 6자리 PIN (OTP Box) UI**: 한눈에 들어오는 6칸 분할 입력 박스로 키보드 입력과 클립보드 붙여넣기를 쾌적하게 지원합니다.

---

## 2. Theme Architecture & Monochrome Tokens

### 2.1 테마 정책 (Theme Policy)
1. **Light & Dark 완벽 지원**: 라이트 모드는 순백(`#FFFFFF`)과 부드러운 오프화이트/징크 캔버스(`#FAFAFA`), 다크 모드는 깊이감 있는 딥 챠콜/블랙(`#09090B`)과 다크 서피스(`#18181B`)로 구성됩니다.
2. **System Auto Sync**: OS의 테마 설정을 자동으로 감지하며, 필요 시 수동 토글이 가능합니다.
3. **High Contrast Typography**: 본문과 제목은 최고 대비의 흑백 텍스트를 사용하여 또렷한 가독성을 제공합니다.

### 2.2 Design Tokens (Dark & White)

#### Surfaces & Backgrounds
| Token | Light Mode | Dark Mode | Usage |
|---|---|---|---|
| `--bg-canvas` | `#FAFAFA` (Zinc 50) | `#09090B` (Zinc 950) | 최상위 앱/화면 캔버스 배경 |
| `--bg-surface` | `#FFFFFF` (Pure White) | `#18181B` (Zinc 900) | 카드, 패널, 모듈 기본 서피스 |
| `--bg-surface-subtle` | `#F4F4F5` (Zinc 100) | `#27272A` (Zinc 800) | 보조 박스, 칩, 호버 배경 |
| `--bg-surface-active` | `#E4E4E7` (Zinc 200) | `#3F3F46` (Zinc 700) | 활성 탭, 프레스 상태 |

#### Borders & Dividers
| Token | Light Mode | Dark Mode | Usage |
|---|---|---|---|
| `--border-subtle` | `#E4E4E7` (Zinc 200) | `rgba(255, 255, 255, 0.10)` | 구분선, 서브 카드 테두리 |
| `--border-card` | `#D4D4D8` (Zinc 300) | `rgba(255, 255, 255, 0.16)` | 메인 카드 윤곽선, 입력 필드 보더 |
| `--border-strong` | `#A1A1AA` (Zinc 400) | `rgba(255, 255, 255, 0.28)` | 포커스 및 모달 테두리 |

#### Typography & Text
| Token | Light Mode | Dark Mode | Usage |
|---|---|---|---|
| `--text-primary` | `#09090B` (Deep Black) | `#FAFAFA` (Crisp White) | 제목, 주요 텍스트, 본문 |
| `--text-secondary` | `#52525B` (Zinc 600) | `#A1A1AA` (Zinc 400) | 설명, 부제목, 라벨 |
| `--text-muted` | `#71717A` (Zinc 500) | `#71717A` (Zinc 500) | 힌트 텍스트, 타임스탬프, 푸터 |
| `--text-dim` | `#A1A1AA` (Zinc 400) | `#52525B` (Zinc 600) | 비활성 아이콘, 비활성 캡션 |

#### Primary & Functional Accents (Monochrome)
| Role | Light Mode | Dark Mode | Purpose |
|---|---|---|---|
| **Action Primary** | `#09090B` (Text: `#FFFFFF`) | `#FAFAFA` (Text: `#09090B`) | 주 액션 버튼, 확인 버튼 |
| **Action Hover** | `#27272A` | `#E4E4E7` | 버튼 호버 상태 |
| **Action Subtle** | `#F4F4F5` (Border: `#E4E4E7`) | `#27272A` (Border: `#3F3F46`) | 보조 액션 버튼 |
| **Status Dot (Active)** | `#09090B` (또는 은은한 녹색 포인트) | `#FAFAFA` (또는 은은한 녹색 포인트) | 스트리밍 상태 표시 |
| **Danger / Stop** | `#18181B` (반전 강조) | `#27272A` (반전 강조) | 공유 중단 및 삭제 버튼 |

---

## 3. Checklist for Ultra-Simple Craftsmanship

- [ ] **1. Simple is Best**: 불필요한 장식, 과도한 배지, 형광 그라디언트를 배제했는가?
- [ ] **2. Dark & White Palette**: 모든 색상 조합이 순수한 흑백 및 징크/그레이스케일 톤으로 통일되었는가?
- [ ] **3. Tabular Numbers**: 모든 실시간 수치(FPS, bps, ms)에 `tabular-nums`가 적용되어 글자 떨림이 없는가?
- [ ] **4. Clean Aspect-Ratio Miniature**: 모니터 목록에 단순 텍스트 대신 비율에 맞춘 미니어처 와이어프레임이 렌더링되는가?
- [ ] **5. Split 6-box OTP PIN**: 모바일 6자리 인증 번호 입력이 6칸 분할 박스로 부드럽게 동작하는가?
- [ ] **6. Big & Crisp Touch Targets**: 최소 44×44pt의 넉넉한 터치 영역과 선명한 터치 피드백을 제공하는가?
