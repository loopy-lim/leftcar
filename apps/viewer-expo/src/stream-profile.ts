export type StreamContentMode = "interactive" | "video";

export const STREAM_PROFILES = [
  {
    id: "latency",
    label: "빠른 반응",
    detail: "1080p 60fps",
    maxWidth: 1920,
    maxHeight: 1080,
    fps: 60,
    contentMode: "interactive",
    hint: "움직임이 많은 화면에 적합",
  },
  {
    id: "video",
    label: "동영상 우선",
    detail: "최대 4K 60fps",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 60,
    allowUpscale: false,
    contentMode: "video",
    hint: "4K 해상도와 큰 화면 변화에 강함",
  },
  {
    id: "balanced",
    label: "균형",
    detail: "1440p 60fps",
    maxWidth: 2560,
    maxHeight: 1440,
    fps: 60,
    contentMode: "interactive",
    hint: "글자 선명도와 반응 속도의 균형",
  },
  {
    id: "clarity",
    label: "선명한 화면",
    detail: "4K 60fps",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 60,
    allowUpscale: true,
    contentMode: "interactive",
    hint: "빠르고 안정적인 Wi-Fi에 적합",
  },
] as const;

export type StreamProfileId = (typeof STREAM_PROFILES)[number]["id"];
export type StreamProfile = (typeof STREAM_PROFILES)[number];
