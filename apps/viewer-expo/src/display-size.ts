/**
 * Virtual display size candidates for the catalog "가상 화면 크기" card.
 *
 * The Host resizes a managed virtual display with logical (point) dimensions
 * plus a HiDPI scale factor; the backing buffer is logical × scale. Every
 * candidate here is expressed in the same terms: `width`/`height` are logical
 * pixels and `scale` is the HiDPI multiplier, so the backing size is
 * width × scale, height × scale.
 */

export interface DisplaySizeCandidate {
  /** Logical (point) width in pixels — even aligned. */
  width: number;
  /** Logical (point) height in pixels — even aligned. */
  height: number;
  /** HiDPI multiplier applied by the Host when backing the display. */
  scale: 1 | 2;
  /** Korean card label shown on the preset button. */
  label: string;
}

export interface DisplaySizeCurrent {
  width: number;
  height: number;
  scale: 1 | 2;
}

/** Smallest logical size the Host accepts for a virtual display. */
const MIN_WIDTH = 640;
const MIN_HEIGHT = 480;
/** Largest logical size offered; also the custom-input ceiling. */
const MAX_WIDTH = 4096;
const MAX_HEIGHT = 4096;
/** A scale-2 candidate must keep at least this logical size (readability). */
const HIDPI_MIN_WIDTH = 1280;
const HIDPI_MIN_HEIGHT = 720;

export function isHiDpiEligible(width: number, height: number): boolean {
  return width >= HIDPI_MIN_WIDTH && height >= HIDPI_MIN_HEIGHT;
}

/**
 * HiDPI scale for a manually entered logical size — the same rule the preset
 * candidates and the host-side matching use: large enough logical sizes map
 * scale 2, anything smaller falls back to 1:1 physical pixels.
 */
export function customSizeScale(width: number, height: number): 1 | 2 {
  return isHiDpiEligible(width, height) ? 2 : 1;
}

function even(value: number): number {
  return Math.floor(value / 2) * 2;
}

function clampCandidate(width: number, height: number): boolean {
  return width >= MIN_WIDTH && height >= MIN_HEIGHT && width <= MAX_WIDTH && height <= MAX_HEIGHT;
}

function candidate(
  width: number,
  height: number,
  label: string,
  current: DisplaySizeCurrent | null,
): DisplaySizeCandidate | null {
  if (!clampCandidate(width, height)) return null;
  // Prefer scale 2 when the logical size is large enough; fall back to 1 so a
  // small logical size still maps 1:1 onto physical pixels.
  const scale: 1 | 2 = isHiDpiEligible(width, height) ? 2 : 1;
  if (current && current.width === width && current.height === height && current.scale === scale) {
    return null;
  }
  return { width, height, scale, label };
}

function tabletCandidate(
  tabletMatch: { width: number; height: number },
  current: DisplaySizeCurrent | null,
): DisplaySizeCandidate | null {
  // Tablet metrics arrive as physical pixels. The matched logical size is
  // physical ÷ 2 (the HiDPI convention used by the rest of the pipeline),
  // even aligned so encoders and the CGVD shim accept the mode.
  const halfWidth = even(tabletMatch.width / 2);
  const halfHeight = even(tabletMatch.height / 2);
  // Host matching uses a landscape-normalized pixel pair so portrait and
  // landscape reports select the same virtual-display mode.
  const width = Math.max(halfWidth, halfHeight);
  const height = Math.min(halfWidth, halfHeight);
  if (!clampCandidate(width, height)) return null;
  const scale: 1 | 2 = isHiDpiEligible(width, height) ? 2 : 1;
  if (current && current.width === width && current.height === height) {
    // Same logical size as now: only re-suggest when the scale actually
    // changes; otherwise the candidate is a no-op.
    return current.scale === scale ? null : { width, height, scale, label: "태블릿 크기" };
  }
  return { width, height, scale, label: "태블릿 크기" };
}

/**
 * Build the preset list for the card. The tablet-matched candidate leads when
 * tablet metrics are available; fixed presets (1080p/1440p/4K) follow with
 * candidates identical to the current size removed.
 */
export function displaySizePresets(
  tabletMatch: { width: number; height: number } | null,
  current: DisplaySizeCurrent | null,
): DisplaySizeCandidate[] {
  const presets: DisplaySizeCandidate[] = [];
  if (tabletMatch) {
    const matched = tabletCandidate(tabletMatch, current);
    if (matched) presets.push(matched);
  }
  const fixed: Array<{ width: number; height: number; label: string }> = [
    { width: 1920, height: 1080, label: "1080p" },
    { width: 2560, height: 1440, label: "1440p" },
    { width: 3840, height: 2160, label: "4K" },
  ];
  for (const preset of fixed) {
    const option = candidate(preset.width, preset.height, preset.label, current);
    if (option) presets.push(option);
  }
  return presets;
}

/**
 * Validate and even-align a manually entered size. Returns null when the
 * entry falls outside the 640×480 … 4096×4096 logical window after alignment.
 */
export function normalizeCustomSize(
  width: number,
  height: number,
): { width: number; height: number } | null {
  if (!Number.isFinite(width) || !Number.isFinite(height)) return null;
  const alignedWidth = even(Math.floor(width));
  const alignedHeight = even(Math.floor(height));
  if (!clampCandidate(alignedWidth, alignedHeight)) return null;
  return { width: alignedWidth, height: alignedHeight };
}
