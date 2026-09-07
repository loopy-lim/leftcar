import { describe, expect, it, vi } from "vitest";
import type { ControlClient } from "./control";
import {
  reconfigurePreparedStream,
  startPreparedStream,
  type StartStreamArgs,
  type StreamLauncher,
} from "./launch-stream";
import type { ActiveStream } from "./catalog-model-types";

const splitAdvertised = [{
  id: "auto",
  label: "자동",
  hint: "자동",
  requiresReconnect: true,
}, {
  id: "splitVertical",
  label: "4K 수직 분할",
  hint: "두 하드웨어 디코더와 Surface를 사용합니다.",
  requiresReconnect: true,
}];

const args: StartStreamArgs = {
  sourceIndex: 0,
  viewerPort: 5010,
  width: 3840,
  height: 2160,
  fps: 60,
  captureBackend: "screenCaptureKit",
  mediaTransport: "udp",
  encoderExperiment: "auto",
  displayName: "Main",
};

function launcher(): StreamLauncher {
  return {
    prepareStream: vi.fn(async () => undefined),
    openStream: vi.fn(async () => "src-5010"),
    cancelPreparedStream: vi.fn(async () => undefined),
  };
}

describe("adaptive stream receipts", () => {
  it("opens the dimensions accepted by Host", async () => {
    const native = launcher();
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command === "startStream") {
          return { session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native" };
        }
        throw new Error(`unexpected command ${command}`);
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    const started = await startPreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      advertisedEncoderExperiments: [{
        id: "auto",
        label: "자동",
        hint: "자동",
        requiresReconnect: true,
      }],
      args,
    });
    expect(started).toMatchObject({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
    expect(native.openStream).toHaveBeenCalledWith(
      5010,
      "192.168.0.134",
      3840,
      2160,
      60,
      "auto",
      "Main",
      true,
      false,
    );
  });

  it("reconfigures the same session and port, then opens the accepted fallback", async () => {
    const native = launcher();
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command === "reconfigureStream") {
          return { session: 42, width: 2560, height: 1440, fps: 60, qualityState: "fallback" };
        }
        throw new Error(`unexpected command ${command}`);
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: {
        ...args,
        port: args.viewerPort,
        session: 42,
        sourceName: "Main",
        viewerIps: ["192.168.0.18"],
        mediaTransport: "udp",
        encoderExperiment: "auto",
        captureBackend: args.captureBackend,
        contentMode: "interactive",
        startedAt: Date.now(),
        sourceTarget: { width: args.width, height: args.height, fps: args.fps },
        activeTarget: { width: args.width, height: args.height, fps: args.fps },
        fallbackTarget: { width: 2560, height: 1440, fps: args.fps },
        qualityState: "native",
      },
      target: { width: 2560, height: 1440, fps: 60 },
      qualityState: "fallback",
    });
    expect(result).toMatchObject({ session: 42, width: 2560, height: 1440, qualityState: "fallback" });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 2560,
      height: 1440,
      fps: 60,
      qualityState: "fallback",
    });
    expect(native.openStream).toHaveBeenCalledWith(
      5010,
      "192.168.0.134",
      2560,
      1440,
      60,
      "auto",
      "Main",
      true,
      false,
    );
  });
});

describe("reconfigure encoder mode transitions", () => {
  const source4K = { width: 3840, height: 2160, fps: 60 };

  function activeStream(overrides: Partial<ActiveStream> = {}): ActiveStream {
    return {
      ...args,
      port: args.viewerPort,
      session: 42,
      sourceName: "Main",
      viewerIps: ["192.168.0.18"],
      mediaTransport: "udp",
      encoderExperiment: "auto",
      captureBackend: args.captureBackend,
      contentMode: "interactive",
      startedAt: Date.now(),
      sourceTarget: source4K,
      // responsive 시작: 1440으로 떠 있고 최대는 4K다.
      activeTarget: { width: 2560, height: 1440, fps: 60 },
      fallbackTarget: null,
      qualityState: "fallback",
      ...overrides,
    };
  }

  function reconfigureControl(accepted: Record<string, unknown>) {
    return {
      request: vi.fn(async (_command: string) => accepted) as ControlClient["request"],
      close: vi.fn(),
    };
  }

  it("never requests a mode transition from a Host without the capability", async () => {
    // 구 capability가 없으면 1440→4K 업시프트도 기존처럼 single auto로
    // 유지된다 — 이전 뷰어가 잘못 split을 요구하지 않던 동작을 유지한다.
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
    expect(native.prepareStream).toHaveBeenCalledWith(5010, "192.168.0.134", "udp", "auto");
    expect(native.openStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", 3840, 2160, 60, "auto", "Main", true, false,
    );
  });

  it("requests splitVertical for a 4K upshift when the capability and advertisement allow it", async () => {
    const native = launcher();
    const control = reconfigureControl({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "splitVertical",
    });
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "splitVertical",
    });
    expect(native.prepareStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", "udp", "splitVertical",
    );
    // 수신기는 Host가 실제 수락한 모드로 열어야 한다.
    expect(native.openStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", 3840, 2160, 60, "splitVertical", "Main", true, false,
    );
    expect(result.encoderExperiment).toBe("splitVertical");
  });

  it("keeps the single path when the Host does not advertise splitVertical", async () => {
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: [splitAdvertised[0]],
    });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
    expect(native.prepareStream).toHaveBeenCalledWith(5010, "192.168.0.134", "udp", "auto");
  });

  it("does not prepare a split receiver for a non-UDP transport", async () => {
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream({ mediaTransport: "usb" }),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.prepareStream).toHaveBeenCalledWith(5010, "192.168.0.134", "usb", "auto");
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
  });

  it("keeps an explicitly chosen single experiment at 4K", async () => {
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream({ encoderExperiment: "adaptiveQp" }),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.prepareStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", "udp", "adaptiveQp",
    );
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
  });

  it("falls back to an explicit auto request when split preparation fails", async () => {
    const native = launcher();
    native.prepareStream = vi.fn(async (_port, _host, _transport, experiment) => {
      if (experiment === "splitVertical") {
        throw new Error("dual decoder unavailable");
      }
    });
    const control = reconfigureControl({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "auto",
    });
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.cancelPreparedStream).toHaveBeenCalledWith(5010, "splitVertical");
    expect(native.prepareStream).toHaveBeenNthCalledWith(
      2, 5010, "192.168.0.134", "udp", "auto",
    );
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "auto",
    });
    expect(result.encoderExperiment).toBe("auto");
  });

  it("cancels the prepared receiver and rethrows when the Host rejects the transition", async () => {
    const native = launcher();
    const control = {
      request: vi.fn(async () => {
        throw new Error("splitVertical not startable");
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    await expect(
      reconfigurePreparedStream({
        control,
        launcher: native,
        host: "192.168.0.134",
        active: activeStream(),
        target: { width: 3840, height: 2160, fps: 60 },
        qualityState: "native",
        reconfigureEncoderExperiment: true,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).rejects.toThrow("splitVertical not startable");
    expect(native.cancelPreparedStream).toHaveBeenCalledWith(5010, "splitVertical");
  });

  it("requests an explicit auto demotion fallback when split preparation fails mid-promotion", async () => {
    // 위 테스트가 준비 실패를 다루므로 여기서는 요청 실패 후 이전 모드가
    // 유지된다는 Host 측 계약(C1)만 단언한다 — 뷰어는 오류를 전파한다.
    const native = launcher();
    const control = {
      request: vi.fn(async (_command: string, requestArgs?: unknown) => {
        expect((requestArgs as { encoderExperiment?: string }).encoderExperiment)
          .toBe("splitVertical");
        throw new Error("mode rejected");
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    await expect(
      reconfigurePreparedStream({
        control,
        launcher: native,
        host: "192.168.0.134",
        active: activeStream(),
        target: { width: 3840, height: 2160, fps: 60 },
        qualityState: "native",
        reconfigureEncoderExperiment: true,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).rejects.toThrow("mode rejected");
  });

  it("keeps the single path for an exact-4K target at 90fps", async () => {
    // 분할 승격은 정확한 4K 60fps 계약에 한정된다 — 90fps 4K 목표는
    // single auto로 유지된다.
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 90, qualityState: "native",
    });
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 90 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 90,
      qualityState: "native",
    });
    expect(native.prepareStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", "udp", "auto",
    );
    expect(result.encoderExperiment).toBe("auto");
  });

  it("keeps the single path for an exact-4K target at 30fps", async () => {
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 30, qualityState: "native",
    });
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 30 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 3840,
      height: 2160,
      fps: 30,
      qualityState: "native",
    });
    expect(native.prepareStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", "udp", "auto",
    );
    expect(result.encoderExperiment).toBe("auto");
  });

  it("keeps the split preparation when the Host accepts the single mode", async () => {
    // Host가 요청(splitVertical)과 다른 모드(auto)를 수락해도 재준비하지
    // 않는다. 준비된 수신기는 재구성 전 바인드 시점에 새 Host 세션의 LCH1
    // 도전 토큰을 이미 캡처했고, 수락 뒤에는 도전이 재전송되지 않으므로
    // 재준비하면 빈 토큰으로 열려 피드백 인증이 깨진다. 단일 모드 열기는
    // 기존 준비의 베이스 포트를 그대로 요구한다.
    const native = launcher();
    const control = reconfigureControl({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "auto",
    });
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.prepareStream).toHaveBeenCalledTimes(1);
    expect(native.prepareStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", "udp", "splitVertical",
    );
    expect(native.cancelPreparedStream).not.toHaveBeenCalled();
    expect(native.openStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", 3840, 2160, 60, "auto", "Main", true, false,
    );
    expect(result.encoderExperiment).toBe("auto");
  });

  it("re-opens without extra preparation when the accepted mode matches", async () => {
    // 새 Host는 보통 명시적 요청 모드를 그대로 수락한다 — 이때 추가
    // prepare/cancel 없이 준비된 모드로 연다.
    const native = launcher();
    const control = reconfigureControl({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "splitVertical",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.prepareStream).toHaveBeenCalledTimes(1);
    expect(native.cancelPreparedStream).not.toHaveBeenCalled();
    expect(native.openStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", 3840, 2160, 60, "splitVertical", "Main", true, false,
    );
  });

  it("opens the prepared mode when a capability Host omits the accepted mode", async () => {
    // 응답에 encoderExperiment가 없으면(구 shim) 준비된 모드를 그대로
    // 연다 — 추가 prepare/cancel은 없다.
    const native = launcher();
    const control = reconfigureControl({
      session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native",
    });
    await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: activeStream(),
      target: { width: 3840, height: 2160, fps: 60 },
      qualityState: "native",
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments: splitAdvertised,
    });
    expect(native.prepareStream).toHaveBeenCalledTimes(1);
    expect(native.cancelPreparedStream).not.toHaveBeenCalled();
    expect(native.openStream).toHaveBeenCalledWith(
      5010, "192.168.0.134", 3840, 2160, 60, "splitVertical", "Main", true, false,
    );
  });

  it("cancels the preserved preparation when the window launch fails after acceptance", async () => {
    // 수락 뒤 재준비하지 않으므로 열기 실패 시 정리 대상은 요청 전에 준비한
    // 모드(split)다. split 취소는 두 포트를 모두 닫아 누수를 남기지 않는다.
    const native = launcher();
    native.openStream = vi.fn(async () => {
      throw new Error("activity launch failed");
    });
    const control = reconfigureControl({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
      encoderExperiment: "auto",
    });
    await expect(
      reconfigurePreparedStream({
        control,
        launcher: native,
        host: "192.168.0.134",
        active: activeStream(),
        target: { width: 3840, height: 2160, fps: 60 },
        qualityState: "native",
        reconfigureEncoderExperiment: true,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).rejects.toThrow("activity launch failed");
    expect(native.prepareStream).toHaveBeenCalledTimes(1);
    expect(native.cancelPreparedStream).toHaveBeenLastCalledWith(
      5010, "splitVertical",
    );
  });
});
