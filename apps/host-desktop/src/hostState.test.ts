import { describe, expect, it } from "vitest";
import { trayStatus, type HostSnapshotView } from "./hostState";

const base: HostSnapshotView = {
  platform: "macos",
  pairingState: "unpaired",
  activeStreamCount: 0,
};

describe("host tray status", () => {
  it("shows capture state prominently", () => {
    expect(trayStatus({ ...base, activeStreamCount: 3 })).toContain("화면 전송 중");
    expect(trayStatus({ ...base, activeStreamCount: 3 })).toContain("3");
  });
});
