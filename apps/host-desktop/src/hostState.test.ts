import { describe, expect, it } from "vitest";
import { canStopAll, trayStatus, type HostSnapshotView } from "./hostState";

const base: HostSnapshotView = {
  hostId: "h1",
  platform: "macos",
  pairingState: "unpaired",
  pairedDevices: [],
  approvedSources: [],
  activeStreamCount: 0,
};

describe("host tray status", () => {
  it("shows capture state prominently", () => {
    expect(trayStatus({ ...base, activeStreamCount: 3 })).toContain("화면 전송 중");
    expect(trayStatus({ ...base, activeStreamCount: 3 })).toContain("3");
  });

  it("stop-all only while capturing", () => {
    expect(canStopAll({ ...base, activeStreamCount: 1 })).toBe(true);
    expect(canStopAll(base)).toBe(false);
  });
});
