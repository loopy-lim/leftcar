import { describe, expect, it } from "vitest";
import { decideOpenAction, openSourceWindow, type SourceRow } from "./HubApp";
import type { StreamWindowLauncherSpec } from "../specs/StreamWindowLauncherSpec";

const source = (over: Partial<SourceRow>): SourceRow => ({
  sourceId: "s1",
  displayName: "IDE",
  kind: "window",
  approved: true,
  available: true,
  ...over,
});

describe("Hub open-source policy", () => {
  it("launches for approved+available new sources", () => {
    expect(decideOpenAction(source({}), new Set()).action).toBe("launch");
  });

  it("focuses the existing window for an already-open source", () => {
    expect(decideOpenAction(source({}), new Set(["s1"])).action).toBe("focus");
  });

  it("disables unapproved or unavailable sources", () => {
    expect(decideOpenAction(source({ approved: false }), new Set()).action).toBe("disabled");
    expect(decideOpenAction(source({ available: false }), new Set()).action).toBe("disabled");
  });

  it("openSourceWindow: unique document per launch (H04 Red)", async () => {
    const launcher: StreamWindowLauncherSpec = {
      open: async (handle, sourceId, instanceId) =>
        `leftcar://stream/${sourceId}?instance=${instanceId}&h=${handle}`,
      focus: async () => true,
      close: async () => {},
    };
    const commands = {
      listRemoteSources: async () => [] as SourceRow[],
      createStreamLaunch: async (id: string) => `leftcar-launch://${id}/abc`,
    };
    const doc = await openSourceWindow(commands, launcher, source({}), new Set());
    expect(doc).toMatch(/^leftcar:\/\/stream\/s1\?instance=/);
    // handle passed through launcher, not the doc
    expect(doc).toContain("h=leftcar-launch://s1/abc");
  });

  it("openSourceWindow: focus path does not create a new task", async () => {
    let opens = 0;
    const launcher: StreamWindowLauncherSpec = {
      open: async () => {
        opens += 1;
        return "doc";
      },
      focus: async () => true,
      close: async () => {},
    };
    const commands = {
      listRemoteSources: async () => [] as SourceRow[],
      createStreamLaunch: async () => "handle",
    };
    await openSourceWindow(commands, launcher, source({}), new Set(["s1"]));
    expect(opens).toBe(0);
  });
});
