export type StreamContentMode = "interactive" | "video";

/**
 * 화질 프로필의 수학적 정의. 사용자에게 보이는 라벨·설명은
 * packages/ui-tokens/src/i18n.ts의 quality* 키가 유일한 원본이다 — 여기에
 * 카피를 두면 i18n과 드리프트된다(디자인 리뷰 5단계에서 제거한 항목).
 */
export const STREAM_PROFILES = [
  {
    id: "latency",
    maxWidth: 1920,
    maxHeight: 1080,
    fps: 60,
    contentMode: "interactive",
  },
  {
    id: "video",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 60,
    allowUpscale: false,
    contentMode: "video",
  },
  {
    id: "balanced",
    maxWidth: 2560,
    maxHeight: 1440,
    fps: 60,
    contentMode: "interactive",
    role: "fallback",
  },
  {
    id: "clarity",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 60,
    allowUpscale: false,
    contentMode: "interactive",
  },
] as const;

export type StreamProfileId = (typeof STREAM_PROFILES)[number]["id"];
export type StreamProfile = (typeof STREAM_PROFILES)[number];

export { is4KResolution } from "./encoder-experiment";
