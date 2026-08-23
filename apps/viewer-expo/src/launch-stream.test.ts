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
};

function harness() {
  const calls: string[] = [];
  const launcher: StreamLauncher = {
    prepareStream: vi.fn(async () => {
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
      calls.push(command === "startStream" ? "start" : "stop");
      return { session: 17 };
    }) as ControlClient["request"],
    close: vi.fn(),
  };
  return { calls, control, launcher };
}

describe("startPreparedStream", () => {
  it("prepares the receiver before Host start and opens only after approval", async () => {
    const { calls, control, launcher } = harness();

    await expect(
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).resolves.toBe(17);
    expect(calls).toEqual(["prepare", "start", "open"]);
  });

  it("cancels the prepared port when Host start fails", async () => {
    const { calls, control, launcher } = harness();
    control.request = vi.fn(async () => {
      calls.push("start");
      throw new Error("reachability failed");
    }) as ControlClient["request"];

    await expect(
      startPreparedStream({ control, launcher, host: "192.168.0.134", args }),
    ).rejects.toThrow("reachability failed");
    expect(calls).toEqual(["prepare", "start", "cancel"]);
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
    expect(calls).toEqual(["prepare", "start", "open", "stop", "cancel"]);
  });
});
