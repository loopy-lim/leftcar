import { describe, expect, it } from "vitest";
import type { ActiveStream } from "./catalog-model-types";
import { streamTargetAfterResize } from "./display-resize";

function stream(overrides: Partial<ActiveStream> = {}): ActiveStream {
  return {
    port: 5010,
    session: 42,
    sourceIndex: 0,
    sourceName: "Main",
    width: 1400,
    height: 876,
    fps: 60,
    sourceTarget: { width: 1400, height: 876, fps: 60 },
    activeTarget: { width: 1400, height: 876, fps: 60 },
    fallbackTarget: null,
    qualityState: "native",
    captureBackend: "screenCaptureKit",
    contentMode: "interactive",
    encoderExperiment: "auto",
    mediaTransport: "udp",
    viewerIps: ["192.168.0.42"],
    startedAt: 1,
    ...overrides,
  };
}

describe("streamTargetAfterResize", () => {
  it("keeps the accepted encoder mode after leaving split resolution", () => {
    const active = stream({ encoderExperiment: "splitVertical", width: 3840, height: 2160 });
    const next = streamTargetAfterResize(
      active,
      { width: 1920, height: 1080, fps: 60 },
      { encoderExperiment: "auto", qualityState: "native" },
    );
    expect(next.encoderExperiment).toBe("auto");
  });

  it("repoints the session target to the accepted resolution", () => {
    const active = stream();
    const next = streamTargetAfterResize(active, {
      width: 1920,
      height: 1080,
      fps: 60,
    }, active);
    expect(next).toMatchObject({ session: 42, width: 1920, height: 1080, fps: 60 });
    // 새 target이 소스 크기이므로 activeTarget도 함께 이동한다.
    expect(next.sourceTarget).toEqual({ width: 1920, height: 1080, fps: 60 });
    expect(next.activeTarget).toEqual({ width: 1920, height: 1080, fps: 60 });
  });

  it("recomputes the fallback target for the new source size", () => {
    const active = stream({
      sourceTarget: { width: 3840, height: 2160, fps: 60 },
      activeTarget: { width: 3840, height: 2160, fps: 60 },
      width: 3840,
      height: 2160,
      fallbackTarget: { width: 2560, height: 1440, fps: 60 },
    });
    const next = streamTargetAfterResize(active, {
      width: 3840,
      height: 2160,
      fps: 60,
    }, active);
    // 4K 소스에는 기존 폴백 정책이 그대로 적용된다.
    expect(next.fallbackTarget).toEqual({ width: 2560, height: 1440, fps: 60 });
  });

  it("keeps unrelated streams untouched", () => {
    const active = stream();
    const other = stream({ session: 43 });
    const remapped = streamTargetAfterResize(other, {
      width: 1920,
      height: 1080,
      fps: 60,
    }, other);
    // 순수 함수는 전달된 스트림만 변환한다 — 세션 매핑·목록 갱신은 호출부 책임.
    expect(remapped).toMatchObject({ session: 43, width: 1920, height: 1080 });
    expect(active.session).toBe(42);
    expect(streamTargetAfterResize(active, { width: 1400, height: 876, fps: 60 }, active).width)
      .toBe(1400);
  });

  it("clears the fallback when the new size needs no downscale", () => {
    const active = stream({
      fallbackTarget: { width: 1280, height: 720, fps: 60 },
    });
    const next = streamTargetAfterResize(active, {
      width: 1920,
      height: 1080,
      fps: 60,
    }, active);
    expect(next.fallbackTarget).toBeNull();
  });
});
