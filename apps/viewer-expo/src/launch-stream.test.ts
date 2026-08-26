import { describe, expect, it, vi } from "vitest";
import type { ControlClient } from "./control";
import {
  startPreparedStream,
  type StartStreamArgs,
  type StreamLauncher,
} from "./launch-stream";

const args: StartStreamArgs = {
  sourceIndex: 1,
  viewerPort: 5003,
  width: 1728,
  height: 1080,
  fps: 60,
  captureBackend: "cgDisplayStream",
  mediaTransport: "auto",
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
  it("waits for the requested AOAP accessory before preparing the USB receiver", async () => {
    const states = [
      { attached: false, controlPort: 0 },
      { attached: true, controlPort: 4141 },
    ];
    vi.stubGlobal("__leftcarUsbRuntime", {
      getUsbNative: () => ({
        getAccessoryState: async () => states.shift() ?? states[0] ?? {
          attached: true,
          controlPort: 4141,
        },
      }),
      subscribeUsbNative: () => ({ remove: () => undefined }),
    });
    const { calls, control, launcher, preparedTransports } = harness();
    control.request = vi.fn(async (command: string) => {
      calls.push(command === "requestUsb" ? "usb" : "start");
      return { session: 17 };
    }) as ControlClient["request"];

    try {
      await expect(
        startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
      ).resolves.toEqual({
        session: 17,
        viewerIps: ["192.168.0.42"],
        mediaTransport: "usb",
      });
      expect(calls).toEqual(["usb", "prepare", "start", "open"]);
      expect(preparedTransports).toEqual(["usb"]);
      expect(control.request).toHaveBeenCalledWith("startStream", {
        ...args,
        mediaTransport: "usb",
        viewerIps: ["192.168.0.42"],
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("prepares the receiver before Host start and opens only after approval", async () => {
    const { calls, control, launcher, preparedTransports } = harness();

    await expect(
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).resolves.toEqual({
      session: 17,
      viewerIps: ["192.168.0.42"],
      mediaTransport: "udp",
    });
    expect(calls).toEqual(["usb", "prepare", "start", "open"]);
    expect(preparedTransports).toEqual(["udp"]);
    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...args,
      mediaTransport: "udp",
      viewerIps: ["192.168.0.42"],
    });
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
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).rejects.toThrow("reachability failed");
    expect(calls).toEqual(["usb", "prepare", "start", "cancel"]);
  });

  it("keeps older native launchers compatible when address discovery is absent", async () => {
    const { control, launcher } = harness();
    delete launcher.getLocalIpv4Addresses;

    await expect(
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).resolves.toEqual({ session: 17, viewerIps: [], mediaTransport: "udp" });
    expect(control.request).toHaveBeenCalledWith("startStream", {
      ...args,
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
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).rejects.toThrow("activity launch failed");
    expect(calls).toEqual(["usb", "prepare", "start", "open", "stop", "cancel"]);
  });
});
