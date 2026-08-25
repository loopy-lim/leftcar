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

export function resolveStreamResolution(
  source: StreamResolutionSource,
  profile: StreamResolutionProfile,
) {
  const profileScale = Math.min(
    profile.maxWidth / Math.max(1, source.width),
    profile.maxHeight / Math.max(1, source.height),
  );
  const scale = profile.allowUpscale ? profileScale : Math.min(1, profileScale);
  return {
    width: Math.max(2, Math.floor((source.width * scale) / 2) * 2),
    height: Math.max(2, Math.floor((source.height * scale) / 2) * 2),
    fps: profile.fps,
  };
}
