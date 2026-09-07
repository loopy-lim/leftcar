/**
 * Session resolution candidates for the catalog "화면 해상도" card.
 *
 * Every candidate is a plain encoder resolution: applying one reconfigures the
 * live session through `reconfigureStream`.
 */

export interface DisplaySizeCandidate {
  width: number;
  height: number;
  /** Korean card label shown on the preset button. */
  label: string;
}

/** Smallest size the Host accepts for a session resolution. */
const MIN_WIDTH = 640;
const MIN_HEIGHT = 480;
/** Largest size offered; also the custom-input ceiling. */
const MAX_WIDTH = 4096;
const MAX_HEIGHT = 4096;

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
  current: { width: number; height: number } | null,
): DisplaySizeCandidate | null {
  if (!clampCandidate(width, height)) return null;
  if (current && current.width === width && current.height === height) {
    return null;
  }
  return { width, height, label };
}

/**
 * Build the preset list for the card: fixed presets (1080p/1440p/4K) with the
 * candidate identical to the current size removed.
 */
export function displaySizePresets(
  current: { width: number; height: number } | null,
): DisplaySizeCandidate[] {
  const presets: DisplaySizeCandidate[] = [];
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
 * entry falls outside the 640×480 … 4096×4096 window after alignment.
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
