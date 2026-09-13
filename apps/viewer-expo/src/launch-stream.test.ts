import { beforeEach, describe, expect, it, vi } from "vitest";
import { setRandomSource } from "./secure-channel";

// 미디어 키 경로를 결정적으로 고정한다(0..31 → base64url).
setRandomSource((length) => Uint8Array.from({ length }, (_, i) => i));
import { setCurrentLanguage } from "./language-store";
import type { ControlClient } from "./control";
import {
  reconfigurePreparedStream,
  replaceRestartedStreamState,
  startPreparedStream,
  type StartStreamArgs,
  type StreamLauncher,
} from "./launch-stream";
import type { EncoderExperimentInfo } from "./encoder-experiment";
import type { AdaptiveQualityState } from "./adaptive-resolution";
import type { ActiveStream } from "./catalog-model-types";
import { STREAM_PROFILES } from "./stream-profile";

// 런처 인자에 현재 언어가 포함되므로 테스트에서는 한국어로 고정한다.
setCurrentLanguage("ko");
import { resolveStreamResolution } from "./stream-resolution";

const advertisedUdpStability = {
  version: 1,
  profiles: ["auto", "responsive", "balanced", "stable"],
  burstDatagramOptions: [2, 4, 8],
  fecParityOptions: [2, 4],
  adaptivePacing: true,
  requiresReconnect: true,
};

const advertisedEncoderExperiments: EncoderExperimentInfo[] = [
  {
    id: "auto",
    label: "자동",
    hint: "호스트가 인코더 경로를 선택합니다.",
    requiresReconnect: true,
  },
  {
    id: "adaptiveQp",
    label: "적응형 QP",
    hint: "화면 변화에 맞춰 QP를 조절합니다.",
    requiresReconnect: true,
  },
  {
    id: "splitVertical",
    label: "4K 수직 분할",
    hint: "두 하드웨어 디코더와 Surface를 사용합니다.",
    requiresReconnect: true,
  },
];

const args: StartStreamArgs = {
  sourceIndex: 1,
  viewerPort: 5003,
  width: 3840,
  height: 2160,
  fps: 60,
  captureBackend: "cgDisplayStream",
  mediaTransport: "auto",
  encoderExperiment: "adaptiveQp",
};

function harness() {
  const calls: string[] = [];
  const preparedTransports: string[] = [];
  const launcher: StreamLauncher = {
    getLocalIpv4Addresses: vi.fn(async () => ["192.168.0.42", "192.168.0.42"]),
    prepareStream: vi.fn(async (_port, _host, transport) => {
      preparedTransports.push(transport);
      calls.push("prepare");
    }),
    openStream: vi.fn(async () => {
      calls.push("open");
      return "src-5003";
    }),
    cancelPreparedStream: vi.fn(async () => {
      calls.push("cancel");
    }),
  };
  const control: ControlClient = {
    request: vi.fn(async (command: string) => {
      if (command === "requestUsb") {
        calls.push("usb");
        throw new Error("USB unavailable in test");
      }
      calls.push(command === "startStream" ? "start" : "stop");
      return { session: 17 };
    }) as ControlClient["request"],
    close: vi.fn(),
  };
  return { calls, control, launcher, preparedTransports };
}

describe("startPreparedStream", () => {
  it("sends only a negotiated UDP selection and includes Viewer capabilities", async () => {
    const { control, launcher } = harness();
    const stableArgs: StartStreamArgs = {
      ...args,
      mediaTransport: "udp",
      udpStability: { profile: "stable" },
    };

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      advertisedUdpStabilityCapabilities: advertisedUdpStability,
      args: stableArgs,
    });

    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...stableArgs,
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      viewerIps: ["192.168.0.42"],
      udpStability: {
        profile: "stable",
        viewer: {
          version: 1,
          maxFecParityShards: 4,
          splitFeedbackBytes: 120,
        },
      },
    });
  });

  it("omits the new request for a legacy Host", async () => {
    const { control, launcher } = harness();
    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args: {
        ...args,
        mediaTransport: "udp",
        udpStability: { profile: "stable" },
      },
    });

    const sent = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls.find(([command]) =>
      command === "startStream"
    )?.[1] as Record<string, unknown>;
    expect(sent).not.toHaveProperty("udpStability");
  });

  it("waits for the requested AOAP accessory before preparing the USB receiver", async () => {
    const states = [
      { attached: false, controlPort: 0 },
      { attached: true, controlPort: 4141 },
    ];
    const previousUsbRuntime = globalThis.__leftcarUsbRuntime;
    globalThis.__leftcarUsbRuntime = {
      getUsbNative: () => ({
        getAccessoryState: async () => states.shift() ?? states[0] ?? {
          attached: true,
          controlPort: 4141,
        },
      }),
      subscribeUsbNative: () => ({ remove: () => undefined }),
    };
    const { calls, control, launcher, preparedTransports } = harness();
    control.request = vi.fn(async (command: string) => {
      calls.push(command === "requestUsb" ? "usb" : "start");
      return { session: 17 };
    }) as ControlClient["request"];

    try {
      await expect(
        startPreparedStream({
          control,
          launcher,
          host: "192.168.0.134",
          advertisedEncoderExperiments,
          args,
        }),
      ).resolves.toEqual({
        session: 17,
        balancedPresentation: false,
      opusAudio: false,
        viewerIps: ["192.168.0.42"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "usb",
        encoderExperiment: "adaptiveQp",
      });
      expect(calls).toEqual(["usb", "prepare", "start", "open"]);
      expect(preparedTransports).toEqual(["usb"]);
      expect(control.request).toHaveBeenCalledWith("startStream", {
        ...args,
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "usb",
        viewerIps: ["192.168.0.42"],
      });
    } finally {
      globalThis.__leftcarUsbRuntime = previousUsbRuntime;
    }
  });

  it("prepares the receiver before Host start and opens only after approval", async () => {
    const { calls, control, launcher, preparedTransports } = harness();

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args,
      }),
    ).resolves.toEqual({
      session: 17,
      balancedPresentation: false,
      opusAudio: false,
      viewerIps: ["192.168.0.42"],
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      encoderExperiment: "adaptiveQp",
    });
    expect(calls).toEqual(["usb", "prepare", "start", "open"]);
    expect(preparedTransports).toEqual(["udp"]);
    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...args,
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      viewerIps: ["192.168.0.42"],
    });
  });

  it("keeps a capability-backed split selection on direct UDP", async () => {
    const { control, launcher } = harness();
    const splitArgs: StartStreamArgs = {
      ...args,
      mediaTransport: "auto",
      encoderExperiment: "splitVertical",
    };

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: splitArgs,
      }),
    ).resolves.toMatchObject({
      mediaTransport: "udp",
      encoderExperiment: "splitVertical",
    });
    expect(launcher.prepareStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      "udp",
      "splitVertical",
      "ko",
      "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
    );
    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "splitVertical",
      undefined,
      false,
      true,
      "ko",
      true,
    );
  });

  it("surfaces preparation failures for an explicitly selected split path", async () => {
    const { control, launcher } = harness();
    launcher.prepareStream = vi.fn(async () => {
      throw new Error("dual decoder unavailable");
    });

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: {
          ...args,
          mediaTransport: "udp",
          encoderExperiment: "splitVertical",
        },
      }),
    ).rejects.toThrow("dual decoder unavailable");
    expect(launcher.prepareStream).toHaveBeenCalledTimes(1);
    expect(control.request).not.toHaveBeenCalledWith(
      "startStream",
      expect.anything(),
    );
  });

  it("forwards displayName to launcher when opening stream", async () => {
    const { control, launcher } = harness();
    const namedArgs: StartStreamArgs = {
      ...args,
      displayName: "LG UltraFine (1)",
    };

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args: namedArgs,
    });

    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "adaptiveQp",
      "LG UltraFine (1)",
      false,
      true,
      "ko",
      true,
    );
  });

  it("forwards the FPS overlay preference to the native stream window", async () => {
    const { control, launcher } = harness();

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args: { ...args, showFps: false },
    });

    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "adaptiveQp",
      undefined,
      false,
      true,
      "ko",
      true,
    );
  });

  it("forwards the local cursor opt-in and defaults it to false for the native arg count", async () => {
    const { control, launcher } = harness();

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args: { ...args, localCursor: true },
    });

    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "adaptiveQp",
      undefined,
      false,
      true,
      "ko",
      true,
    );
  });

  it("forwards the local audio opt-out and defaults it to true for the native arg count", async () => {
    const { control, launcher } = harness();

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args: { ...args, localAudio: false },
    });

    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "adaptiveQp",
      undefined,
      false,
      true,
      "ko",
      false,
    );
  });

  it("cancels the prepared port when Host start fails", async () => {
    const { calls, control, launcher } = harness();
    control.request = vi.fn(async (command: string) => {
      if (command === "requestUsb") {
        calls.push("usb");
        throw new Error("USB unavailable in test");
      }
      calls.push("start");
      throw new Error("reachability failed");
    }) as ControlClient["request"];

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args,
      }),
    ).rejects.toThrow("reachability failed");
    expect(calls).toEqual(["usb", "prepare", "start", "cancel"]);
  });

  it("keeps the legacy startStream payload free of removed virtual display fields", async () => {
    const { control, launcher } = harness();

    await startPreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      advertisedEncoderExperiments,
      args,
    });

    const sent = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls.find(([command]) =>
      command === "startStream"
    )?.[1] as Record<string, unknown>;
    expect(sent).not.toHaveProperty("viewerDisplay");
    expect(sent).not.toHaveProperty("virtualDisplayId");
  });

  it("keeps older native launchers compatible when address discovery is absent", async () => {
    const { control, launcher } = harness();
    delete launcher.getLocalIpv4Addresses;

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args,
      }),
    ).resolves.toEqual({
      session: 17,
      balancedPresentation: false,
      opusAudio: false,
      viewerIps: [],
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      encoderExperiment: "adaptiveQp",
    });
    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...args,
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
    });
  });

  it("stops the Host session and cancels preparation when window launch fails", async () => {
    const { calls, control, launcher } = harness();
    launcher.openStream = vi.fn(async () => {
      calls.push("open");
      throw new Error("activity launch failed");
    });

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args,
      }),
    ).rejects.toThrow("activity launch failed");
    expect(calls).toEqual(["usb", "prepare", "start", "open", "stop", "cancel"]);
  });

  it("resolves a 4K-selected experiment to auto at the actual 1440p request boundary", async () => {
    const profile = STREAM_PROFILES.find((candidate) => candidate.id === "video");
    expect(profile).toBeDefined();
    const fitted = resolveStreamResolution(
      { width: 2560, height: 1440 },
      profile ?? STREAM_PROFILES[0],
    );
    const sub4KArgs: StartStreamArgs = { ...args, ...fitted };
    const { control, launcher } = harness();

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: sub4KArgs,
      }),
    ).resolves.toEqual({
      session: 17,
      balancedPresentation: false,
      opusAudio: false,
      viewerIps: ["192.168.0.42"],
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      encoderExperiment: "auto",
    });
    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...sub4KArgs,
      encoderExperiment: "auto",
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      mediaTransport: "udp",
      viewerIps: ["192.168.0.42"],
    });
  });

  it("automatically selects split AVE for a capability-backed 4K UDP stream", async () => {
    const { control, launcher } = harness();
    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: {
          ...args,
          mediaTransport: "udp",
          encoderExperiment: "auto",
        },
      }),
    ).resolves.toMatchObject({
      mediaTransport: "udp",
      encoderExperiment: "splitVertical",
    });
    expect(control.request).toHaveBeenCalledWith("startStream", expect.objectContaining({
      mediaTransport: "udp",
      encoderExperiment: "splitVertical",
    }));
    expect(launcher.prepareStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      "udp",
      "splitVertical",
      "ko",
      "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
    );
  });

  it("falls back to the single hardware path when automatic split preparation is unavailable", async () => {
    const { control, launcher } = harness();
    launcher.prepareStream = vi.fn(async (_port, _host, _transport, experiment) => {
      if (experiment === "splitVertical") {
        throw new Error("dual decoder unavailable");
      }
    });

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: {
          ...args,
          mediaTransport: "udp",
          encoderExperiment: "auto",
        },
      }),
    ).resolves.toMatchObject({
      mediaTransport: "udp",
      encoderExperiment: "auto",
    });
    expect(launcher.prepareStream).toHaveBeenNthCalledWith(
      1,
      5003,
      "192.168.0.134",
      "udp",
      "splitVertical",
      "ko",
      "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
    );
    expect(launcher.cancelPreparedStream).toHaveBeenCalledWith(
      5003,
      "splitVertical",
    );
    expect(launcher.prepareStream).toHaveBeenNthCalledWith(
      2,
      5003,
      "192.168.0.134",
      "udp",
      "auto",
      "ko",
      "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
    );
    expect(control.request).toHaveBeenCalledWith("startStream", expect.objectContaining({
      mediaTransport: "udp",
      encoderExperiment: "auto",
    }));
  });

  it("re-prepares the split receiver when a terminated split stream is restored", async () => {
    // 이유 5(render stalled)로 소켓을 잃은 분할 스트림은 React/Host 재준비
    // 경로로 복구된다. 복구 인자는 활성 스트림의 split 모드를 그대로 운반하며
    // startStream 전에 수신기를 다시 준비해야 한다 — 다시 바인드한 수신기는
    // 새 Host 세션의 LCH1 도전 토큰을 새로 캡처하기 때문이다.
    const { calls, control, launcher } = harness();

    await expect(
      startPreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        advertisedEncoderExperiments,
        args: {
          ...args,
          mediaTransport: "auto",
          encoderExperiment: "splitVertical",
        },
      }),
    ).resolves.toMatchObject({
      mediaTransport: "udp",
      encoderExperiment: "splitVertical",
    });
    expect(launcher.prepareStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      "udp",
      "splitVertical",
      "ko",
      "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
    );
    expect(calls.indexOf("prepare")).toBeLessThan(calls.indexOf("start"));
  });

  it("uses one replacement boundary for successful restore and transport results", () => {
    const endedUnownedSessions: number[] = [];
    const streams = [
      {
        session: 1,
        captureBackend: "cgDisplayStream",
        viewerIps: ["10.0.0.1"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "udp" as const,
        encoderExperiment: "auto" as const,
        startedAt: 1,
        width: 3840,
        height: 2160,
      },
      {
        session: 2,
        captureBackend: "cgDisplayStream",
        viewerIps: ["10.0.0.2"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "usb" as const,
        encoderExperiment: "adaptiveQp" as const,
        startedAt: 2,
        width: 2560,
        height: 1440,
      },
    ];
    const restored4K = replaceRestartedStreamState(
      streams,
      1,
      {
        session: 11,
        captureBackend: "screenCaptureKit",
        viewerIps: ["10.0.0.11"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "usb",
        encoderExperiment: "adaptiveQp",
      },
      101,
      (session) => endedUnownedSessions.push(session),
    );
    const replacedSub4K = replaceRestartedStreamState(
      restored4K,
      2,
      {
        session: 22,
        captureBackend: "screenCaptureKit",
        viewerIps: ["10.0.0.22"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "udp",
        encoderExperiment: "auto",
      },
      202,
      (session) => endedUnownedSessions.push(session),
    );

    expect(replacedSub4K).toEqual([
      {
        ...streams[0],
        session: 11,
        captureBackend: "screenCaptureKit",
        viewerIps: ["10.0.0.11"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "usb",
        encoderExperiment: "adaptiveQp",
        startedAt: 101,
      },
      {
        ...streams[1],
        session: 22,
        captureBackend: "screenCaptureKit",
        viewerIps: ["10.0.0.22"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "udp",
        encoderExperiment: "auto",
        startedAt: 202,
      },
    ]);
    expect(endedUnownedSessions).toEqual([]);
  });

  it("ends the successful reconnect when its prior stream disappeared before replacement", () => {
    const endedUnownedSessions: number[] = [];

    const streams = replaceRestartedStreamState(
      [],
      1,
      {
        session: 11,
        captureBackend: "screenCaptureKit",
        viewerIps: ["10.0.0.11"],
        mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        mediaTransport: "udp",
        encoderExperiment: "auto",
      },
      101,
      (session) => endedUnownedSessions.push(session),
    );

    expect(streams).toEqual([]);
    expect(endedUnownedSessions).toEqual([11]);
  });
});

describe("reconfigurePreparedStream", () => {
  const fourKTarget = { width: 3840, height: 2160, fps: 60 };
  const sub4KTarget = { width: 2560, height: 1440, fps: 60 };
  const nativeState: AdaptiveQualityState = "native";

  function activeStream(overrides: Partial<ActiveStream> = {}): ActiveStream {
    return {
      port: 5003,
      session: 31,
      sourceIndex: 1,
      sourceName: "LG UltraFine (1)",
      width: 3840,
      height: 2160,
      fps: 60,
      sourceTarget: fourKTarget,
      activeTarget: fourKTarget,
      fallbackTarget: null,
      qualityState: "native",
      captureBackend: "cgDisplayStream",
      contentMode: "interactive",
      encoderExperiment: "auto",
      mediaTransport: "udp",
      viewerIps: ["192.168.0.42"],
      mediaKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
      startedAt: 1,
      ...overrides,
    };
  }

  function reconfigureHarness(acceptedExperiments: string[]) {
    const order: string[] = [];
    let reconfigureCount = 0;
    const launcher: StreamLauncher = {
      prepareStream: vi.fn(async (_port, _host, _transport, experiment) => {
        order.push(`prepare:${experiment}`);
      }),
      openStream: vi.fn(async () => {
        order.push("open");
        return "src-5003";
      }),
      cancelPreparedStream: vi.fn(async (_port, experiment) => {
        order.push(`cancel:${experiment}`);
      }),
    };
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command !== "reconfigureStream") throw new Error("unexpected");
        const encoderExperiment = acceptedExperiments[
          Math.min(reconfigureCount, acceptedExperiments.length - 1)
        ];
        reconfigureCount += 1;
        order.push(`reconfigure:${encoderExperiment}`);
        return {
          session: 31,
          width: 3840,
          height: 2160,
          fps: 60,
          qualityState: "native",
          ...(encoderExperiment ? { encoderExperiment } : {}),
        };
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    return { order, control, launcher };
  }

  it("keeps the existing preparation when the accepted mode matches the prepared mode", async () => {
    const { order, control, launcher } = reconfigureHarness(["splitVertical"]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target: fourKTarget,
      qualityState: nativeState,
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments,
    });
    expect(started.encoderExperiment).toBe("splitVertical");
    expect(order).toEqual(["prepare:splitVertical", "reconfigure:splitVertical", "open"]);
    expect(launcher.cancelPreparedStream).not.toHaveBeenCalled();
  });

  it("re-prepares split and re-requests the reconfiguration when split is accepted against a single preparation", async () => {
    // Promotion fallback: the split preflight failed locally so the viewer
    // prepared auto and requested auto, but the Host still selected split.
    // Binding split listeners AFTER that reconfiguration would hand the
    // renderer an empty challenge token, so the viewer must reconfigure
    // again and let the fresh listeners capture the new challenge.
    const { order, control, launcher } = reconfigureHarness(["splitVertical", "splitVertical"]);
    const prepareResults: Array<Record<string, Error>> = [];
    launcher.prepareStream = vi.fn(async (_port, _host, _transport, experiment) => {
      order.push(`prepare:${experiment}`);
      if (experiment === "splitVertical" && prepareResults.length === 0) {
        prepareResults.push({});
        throw new Error("dual decoder unavailable");
      }
      prepareResults.push({});
    });
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target: fourKTarget,
      qualityState: nativeState,
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments,
    });
    expect(started.encoderExperiment).toBe("splitVertical");
    expect(order).toEqual([
      "prepare:splitVertical",
      "cancel:splitVertical",
      "prepare:auto",
      "reconfigure:splitVertical",
      "cancel:auto",
      "prepare:splitVertical",
      "reconfigure:splitVertical",
      "open",
    ]);
    const requests = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls.filter(([command]) => command === "reconfigureStream");
    expect(requests[1][1]).toMatchObject({ encoderExperiment: "splitVertical" });
  });

  it("fails loudly when the second reconfiguration still does not land on split", async () => {
    const { order, control, launcher } = reconfigureHarness(["splitVertical", "auto"]);
    let splitAttempts = 0;
    launcher.prepareStream = vi.fn(async (_port, _host, _transport, experiment) => {
      order.push(`prepare:${experiment}`);
      if (experiment === "splitVertical" && splitAttempts++ === 0) {
        throw new Error("dual decoder unavailable");
      }
    });
    await expect(
      reconfigurePreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        active: activeStream(),
        target: fourKTarget,
        qualityState: nativeState,
        reconfigureEncoderExperiment: true,
        advertisedEncoderExperiments,
      }),
    ).rejects.toThrow("leftcar:errSplitEncodeRejected");
    expect(order[order.length - 1]).toBe("cancel:splitVertical");
    expect(launcher.openStream).not.toHaveBeenCalled();
  });

  it("never requests a mode transition without the host capability", async () => {
    const { order, control, launcher } = reconfigureHarness([undefined as unknown as string]);
    const sent = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target: fourKTarget,
      qualityState: nativeState,
      advertisedEncoderExperiments,
    });
    expect(sent.encoderExperiment).toBe("auto");
    expect(order).toEqual(["prepare:auto", "reconfigure:undefined", "open"]);
    const request = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls[0][1] as Record<string, unknown>;
    expect(request).not.toHaveProperty("encoderExperiment");
  });

  it("demotes a split stream to the single path for a sub-4K target", async () => {
    const { order, control, launcher } = reconfigureHarness(["auto"]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream({
        encoderExperiment: "splitVertical",
        activeTarget: fourKTarget,
      }),
      target: sub4KTarget,
      qualityState: nativeState,
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments,
    });
    expect(started.encoderExperiment).toBe("auto");
    expect(order).toEqual(["prepare:auto", "reconfigure:auto", "open"]);
  });

  it("refuses a split promotion that would exceed the decoder budget", async () => {
    // M4/R8: split(2) + two singles(2, one of them this stream) already fill
    // the four-instance budget. The promotion releases this stream's single
    // slot but would still need 3 + 2 = 5, so the viewer keeps the single
    // path and never sends a split request.
    const { order, control, launcher } = reconfigureHarness(["auto"]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream({ width: 2560, height: 1440, activeTarget: sub4KTarget }),
      target: fourKTarget,
      qualityState: nativeState,
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments,
      decoderBudget: {
        currentStreams: [
          { split: true },
          { split: false },
          { split: false },
        ],
      },
    });
    expect(started.encoderExperiment).toBe("auto");
    expect(order).toEqual(["prepare:auto", "reconfigure:auto", "open"]);
    const request = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls[0][1] as Record<string, unknown>;
    expect(request).not.toHaveProperty("encoderExperiment");
  });

  it("releases the promoted stream's own slot before checking the budget", async () => {
    // split(2) + this single(1) = 3. The promotion releases this stream's
    // single slot first, so the projected total is 2 + 2 = 4 — allowed.
    const { order, control, launcher } = reconfigureHarness(["splitVertical"]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream({ width: 2560, height: 1440, activeTarget: sub4KTarget }),
      target: fourKTarget,
      qualityState: nativeState,
      reconfigureEncoderExperiment: true,
      advertisedEncoderExperiments,
      decoderBudget: {
        currentStreams: [
          { split: true },
          { split: false },
        ],
      },
    });
    expect(started.encoderExperiment).toBe("splitVertical");
    expect(order).toEqual(["prepare:splitVertical", "reconfigure:splitVertical", "open"]);
  });

  it("switches the source on the existing prepared listener before reconfiguring", async () => {
    // Cheap display switch (R7): the receiver must be bound BEFORE the
    // reconfigure request so it captures the replacement backend's single
    // LCH1 challenge, on the same port the open window already listens on.
    const order: string[] = [];
    const launcher: StreamLauncher = {
      prepareStream: vi.fn(async () => {
        order.push("prepare");
      }),
      openStream: vi.fn(async () => {
        order.push("open");
        return "src-5003";
      }),
      cancelPreparedStream: vi.fn(async () => {
        order.push("cancel");
      }),
    };
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command !== "reconfigureStream") throw new Error("unexpected");
        order.push("reconfigure");
        return {
          session: 31,
          width: 2560,
          height: 1440,
          fps: 60,
          qualityState: "native",
          sourceIndex: 2,
          sourceName: "Side Display",
        };
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    const target = { width: 2560, height: 1440, fps: 60 };
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target,
      qualityState: nativeState,
      sourceIndex: 2,
      reconfigureSource: true,
    });
    expect(order).toEqual(["prepare", "reconfigure", "open"]);
    const request = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls[0][1] as Record<string, unknown>;
    expect(request).toMatchObject({ session: 31, sourceIndex: 2 });
    expect(request).not.toHaveProperty("encoderExperiment");
    // The open window keeps its port and adopts the accepted source name.
    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      2560,
      1440,
      60,
      "auto",
      "Side Display",
      false,
      true,
      "ko",
      true,
    );
    expect(started.sourceIndex).toBe(2);
    expect(started.sourceName).toBe("Side Display");
    expect(launcher.cancelPreparedStream).not.toHaveBeenCalled();
  });

  it("falls back to the requested index when a switch host omits the source echo", async () => {
    const { control, launcher } = reconfigureHarness([undefined as unknown as string]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target: sub4KTarget,
      qualityState: nativeState,
      sourceIndex: 3,
      reconfigureSource: true,
    });
    expect(started.sourceIndex).toBe(3);
    expect(started.sourceName).toBeUndefined();
    // The window name falls back to the stream's existing source; geometry
    // comes from the Host response the harness returns (4K).
    expect(launcher.openStream).toHaveBeenCalledWith(
      5003,
      "192.168.0.134",
      3840,
      2160,
      60,
      "auto",
      "LG UltraFine (1)",
      false,
      true,
      "ko",
      true,
    );
  });

  it("never sends sourceIndex without the reconfigureSource capability", async () => {
    // Older hosts silently ignore unknown fields: sending sourceIndex to one
    // would leave the stream on the old display while the viewer believes the
    // switch happened. Without the capability the field must stay absent.
    const { control, launcher } = reconfigureHarness([undefined as unknown as string]);
    const started = await reconfigurePreparedStream({
      control,
      launcher,
      host: "192.168.0.134",
      active: activeStream(),
      target: sub4KTarget,
      qualityState: nativeState,
      sourceIndex: 3,
    });
    const request = (control.request as unknown as {
      mock: { calls: Array<[string, unknown?]> };
    }).mock.calls[0][1] as Record<string, unknown>;
    expect(request).not.toHaveProperty("sourceIndex");
    expect(started).not.toHaveProperty("sourceIndex");
  });

  it("cancels the prepared receiver and propagates when a source switch fails", async () => {
    // The Host validates a switch before stopping the live stream, so a
    // failure leaves the session untouched; the viewer must release its
    // prepared listener and surface the error (same shape as the split
    // promotion fallback's cancel path).
    const order: string[] = [];
    const launcher: StreamLauncher = {
      prepareStream: vi.fn(async () => {
        order.push("prepare");
      }),
      openStream: vi.fn(async () => {
        order.push("open");
        return "src-5003";
      }),
      cancelPreparedStream: vi.fn(async () => {
        order.push("cancel");
      }),
    };
    const control: ControlClient = {
      request: vi.fn(async () => {
        order.push("reconfigure");
        throw new Error("no such display 5");
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    await expect(
      reconfigurePreparedStream({
        control,
        launcher,
        host: "192.168.0.134",
        active: activeStream(),
        target: sub4KTarget,
        qualityState: nativeState,
        sourceIndex: 5,
        reconfigureSource: true,
      }),
    ).rejects.toThrow("no such display 5");
    expect(order).toEqual(["prepare", "reconfigure", "cancel"]);
    expect(launcher.openStream).not.toHaveBeenCalled();
  });
});

describe("isStreamPrepareError", () => {
  it("recognizes the native prepare rejection by code and message", async () => {
    const { isStreamPrepareError } = await import("./launch-stream");
    const withCode = Object.assign(new Error("bind failed"), {
      code: "ERR_STREAM_PREPARE",
    });
    expect(isStreamPrepareError(withCode)).toBe(true);
    expect(isStreamPrepareError(new Error("[ERR_STREAM_PREPARE] boom"))).toBe(
      true,
    );
  });

  it("does not claim unrelated failures", async () => {
    const { isStreamPrepareError } = await import("./launch-stream");
    expect(isStreamPrepareError(new Error("network unreachable"))).toBe(false);
    const capability = Object.assign(new Error("no codec"), {
      code: "ERR_SPLIT_DECODER_CAPABILITY",
    });
    expect(isStreamPrepareError(capability)).toBe(false);
    expect(isStreamPrepareError(undefined)).toBe(false);
  });

});

describe("presentation method negotiation", () => {
  it("keeps the old native eleven-argument contract and reports immediate fallback", async () => {
    const {control,launcher}=harness();
    const result=await startPreparedStream({control,launcher,host:"192.168.0.134",advertisedEncoderExperiments,args:{...args,balancedPresentation:true}});
    expect(vi.mocked(launcher.openStream).mock.calls[0]).toHaveLength(11);
    expect(result.balancedPresentation).toBe(false);
    const request=vi.mocked(control.request).mock.calls.find(([command])=>command==="startStream")?.[1];
    expect(request).not.toHaveProperty("balancedPresentation");
  });
  it("passes the optional mode through the new native launch and reconfigure boundary", async () => {
    const {control,launcher}=harness();
    launcher.openStreamWithPresentation=vi.fn(async()=>"src-5003");
    const result=await startPreparedStream({control,launcher,host:"192.168.0.134",advertisedEncoderExperiments,args:{...args,balancedPresentation:true}});
    expect(result.balancedPresentation).toBe(true);
    expect(launcher.openStream).not.toHaveBeenCalled();
    expect(vi.mocked(launcher.openStreamWithPresentation).mock.calls[0]).toHaveLength(12);
    expect(vi.mocked(launcher.openStreamWithPresentation).mock.calls[0][11]).toBe(true);
    const accepted={session:31,width:3840,height:2160,fps:60,qualityState:"native" as const};
    const reconfigure={...control,request:vi.fn(async()=>accepted) as ControlClient["request"]};
    const active: ActiveStream = {...result,...accepted,port:5003,sourceIndex:1,sourceName:"Main",
      sourceTarget:accepted,activeTarget:accepted,fallbackTarget:null,captureBackend:"cgDisplayStream",
      contentMode:"interactive",startedAt:1,balancedPresentation:true};
    const next=await reconfigurePreparedStream({control:reconfigure,launcher,host:"192.168.0.134",active,target:accepted,qualityState:"native"});
    expect(next.balancedPresentation).toBe(true);
    expect(vi.mocked(launcher.openStreamWithPresentation).mock.calls.at(-1)?.[11]).toBe(true);
    delete launcher.openStreamWithPresentation;
    const legacy=await reconfigurePreparedStream({control:reconfigure,launcher,host:"192.168.0.134",active,target:accepted,qualityState:"native"});
    expect(legacy.balancedPresentation).toBe(false);
    expect(vi.mocked(launcher.openStream).mock.calls.at(-1)).toHaveLength(11);
  });
});
