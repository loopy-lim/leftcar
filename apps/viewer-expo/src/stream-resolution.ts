export interface StreamResolutionSource {
  width: number;
  height: number;
}

export interface StreamResolutionProfile {
  maxWidth: number;
  maxHeight: number;
  fps: number;
  allowUpscale?: boolean;
}

export interface StreamResolutionBudget {
  /** Long-side width cap in landscape orientation (mirrored for portrait sources). */
  maxWidth?: number;
  /** Short-side height anchor in landscape orientation (mirrored for portrait sources). */
  maxHeight?: number;
  /** Hard ceiling on encoded pixels, applied after the box fit. */
  maxPixels?: number;
  allowUpscale?: boolean;
}

/** Floors to an even, codec-safe dimension with a readability floor of two. */
export function evenCodecDimension(value: number): number {
  return Math.max(2, Math.floor(value / 2) * 2);
}

/**
 * Aspect-preserving fit of a logical source size into a resolution budget.
 * Portrait sources are normalized to landscape space so `maxWidth` always
 * bounds the long side and `maxHeight` anchors the short side; the result is
 * mirrored back. `maxPixels` is orientation-independent and shrinks the fit
 * proportionally when the box fit would still exceed the pixel ceiling.
 */
export function fitStreamResolution(
  source: StreamResolutionSource,
  budget: StreamResolutionBudget,
): { width: number; height: number } {
  const portrait = source.height > source.width;
  const longSide = portrait ? source.height : source.width;
  const shortSide = portrait ? source.width : source.height;

  let scale = Math.min(
    budget.maxWidth !== undefined
      ? budget.maxWidth / Math.max(1, longSide)
      : Infinity,
    budget.maxHeight !== undefined
      ? budget.maxHeight / Math.max(1, shortSide)
      : Infinity,
    budget.allowUpscale ? Infinity : 1,
  );
  let width = longSide * scale;
  let height = shortSide * scale;
  if (budget.maxPixels !== undefined && width * height > budget.maxPixels) {
    scale = Math.sqrt(budget.maxPixels / (width * height));
    width *= scale;
    height *= scale;
  }

  const evenWidth = evenCodecDimension(width);
  const evenHeight = evenCodecDimension(height);
  return portrait
    ? { width: evenHeight, height: evenWidth }
    : { width: evenWidth, height: evenHeight };
}

export function resolveStreamResolution(
  source: StreamResolutionSource,
  profile: StreamResolutionProfile,
) {
  const fitted = fitStreamResolution(source, {
    maxWidth: profile.maxWidth,
    maxHeight: profile.maxHeight,
    allowUpscale: profile.allowUpscale,
  });
  return {
    width: fitted.width,
    height: fitted.height,
    fps: profile.fps,
  };
}
