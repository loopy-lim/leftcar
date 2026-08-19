# Leftcar Design System (DESIGN.md)

> **Inspiration**: Raycast (Sleek dark chrome & radiant accents) + Linear (Precision minimal craftsmanship) + Vercel (High-contrast typography & Geist/Inter aesthetics).
> **Target Platforms**: Desktop Host (macOS/Windows Tauri Web) & Mobile/XR Viewer (Galaxy XR / Android Expo React Native).

---

## 1. Design Philosophy

Leftcar is a high-performance, sub-30ms low-latency multi-window desktop streaming platform for Desktop and XR devices. The visual language conveys **speed, precision, and engineering craft**:

- **Obsidian Dark Canvas**: Deep, immersive dark tones (`#090D16` / `#0B0F19`) that minimize eye fatigue and let stream content stand out.
- **Precision Hairlines & Glass Surfaces**: Clean translucent cards (`rgba(15, 23, 42, 0.75)`) bordered by crisp hairline strokes (`rgba(255, 255, 255, 0.08)`).
- **Luminescent Accent Glows**: Radiant Indigo (`#6366F1`) for primary actions, Emerald (`#10B981`) for live low-latency pipelines and active streams, and Sky (`#38BDF8`) for telemetry.
- **Monospace Telemetry**: Monospaced metrics for FPS, bitrates, frame latencies, and TCP port mapping.

---

## 2. Color Palette & Design Tokens

### Backgrounds & Surfaces
| Token | Hex / Value | Description |
|---|---|---|
| `bg-canvas` | `#080B11` | Root background for desktop and mobile screens |
| `bg-card` | `#0F172A` | Elevated surface for primary containers and modules |
| `bg-card-subtle` | `#161F33` | Secondary item card or interactive list item |
| `bg-glass` | `rgba(15, 23, 42, 0.72)` | Translucent backdrop with blur filter |

### Borders & Dividers
| Token | Hex / Value | Description |
|---|---|---|
| `border-subtle` | `rgba(255, 255, 255, 0.08)` | Standard separator hairline |
| `border-card` | `rgba(255, 255, 255, 0.12)` | Card outline and container border |
| `border-accent` | `rgba(99, 102, 241, 0.35)` | Focused, active, or highlighted element border |

### Accents & Semantic Colors
| Token | Hex / Value | Usage |
|---|---|---|
| `accent-primary` | `#6366F1` (Indigo 500) | Primary CTA buttons, brand badges, active focus |
| `accent-primary-hover` | `#4F46E5` (Indigo 600) | Button hover state |
| `accent-emerald` | `#10B981` (Emerald 500)| Active live streams, connected mDNS host, healthy fps |
| `accent-sky` | `#38BDF8` (Sky 400) | Monospace identifiers, document task tokens |
| `accent-amber` | `#F59E0B` (Amber 500) | Reconnecting, dropped frame warning |
| `accent-rose` | `#EF4444` (Rose 500) | Stream termination, disconnected error |

### Typography & Text
| Token | Value | Usage |
|---|---|---|
| `text-primary` | `#F8FAFC` (Slate 50) | Main headings, card titles, key values |
| `text-secondary` | `#94A3B8` (Slate 400) | Supporting labels, descriptions, metadata |
| `text-muted` | `#64748B` (Slate 500) | Footers, disabled hints, timestamps |
| `text-mono` | `JetBrains Mono, SF Mono, Menlo, monospace` | Ports, IP addresses, FPS, resolution |

---

## 3. Typography Hierarchy

- **Hero / Title**: 24px - 28px, Bold (700), Tracking `-0.02em`
- **Section Heading**: 16px - 18px, SemiBold (600), Tracking `-0.01em`
- **Card Title / Row Name**: 14px - 15px, Medium (500)
- **Body / Subtitle**: 13px - 14px, Regular (400)
- **Meta / Badge / Caption**: 11px - 12px, Medium (500)
- **Monospace Code/Metric**: 12px - 13px, Regular (400)

---

## 4. Component Patterns

### 4.1 Status Pills & Badges
- **Live / Connected**: Emerald green dot with pulsing glow ring (`● CONNECTED / BROADCASTING`), background `rgba(16, 185, 129, 0.12)`, text `#34D399`.
- **Standby / Waiting**: Amber/Blue dot (`● STANDBY / DISCOVERING`), background `rgba(59, 130, 246, 0.12)`.
- **Codec / Spec Chip**: Subtle slate capsule (`60 FPS`, `1920×1080`, `AMediaCodec`), background `#1E293B`, border `#334155`.

### 4.2 Buttons
- **Primary CTA**: Linear gradient from `#6366F1` to `#4F46E5`, rounded 8px - 10px, shadow `0 2px 10px rgba(99, 102, 241, 0.25)`.
- **Secondary / Action**: Dark slate surface (`#1E293B`), hairline border, text `#F8FAFC`, hover brightness `1.15`.
- **Destructive / Stop**: Crimson slate surface (`rgba(239, 68, 68, 0.15)`), border `rgba(239, 68, 68, 0.35)`, text `#FCA5A5`.

### 4.3 Cards & Data Grid
- Elevated dark slate card with rounded corners (`12px` - `16px`).
- Subtle 1px translucent border (`rgba(255, 255, 255, 0.08)`).
- Hover transition: soft border illumination (`rgba(99, 102, 241, 0.3)`) and subtle lift.

### 4.4 Empty States
- Animated radar pulse icon with glowing rings.
- Clear title and contextual subtitle explaining how to connect from mobile or XR headset.

---

## 5. Implementation Guardrails

- **Consistent System Theme**: Dark mode default for all platforms.
- **Zero Layout Shift**: Fixed metric cell widths and pre-allocated card heights.
- **High Visual Contrast**: Text must meet WCAG AAA contrast against dark backgrounds.
- **Native Feel**: Tauri desktop uses native macOS/Windows window decorations and smooth web transitions; React Native mobile uses fluid safe-area insets and touch feedback.
