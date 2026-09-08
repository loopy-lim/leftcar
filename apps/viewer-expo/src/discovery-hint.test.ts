import { describe, expect, it } from "vitest";

import {
  DISCOVERY_HINT_DELAY_MS,
  shouldShowDiscoveryHint,
} from "./discovery-hint";

describe("shouldShowDiscoveryHint", () => {
  it("does not show while the picker is still warming up", () => {
    expect(
      shouldShowDiscoveryHint({ hostCount: 0, elapsedMs: 0 }),
    ).toBe(false);
    expect(
      shouldShowDiscoveryHint({
        hostCount: 0,
        elapsedMs: DISCOVERY_HINT_DELAY_MS - 1,
      }),
    ).toBe(false);
  });

  it("shows once the picker has been open with no hosts for a moment", () => {
    expect(
      shouldShowDiscoveryHint({
        hostCount: 0,
        elapsedMs: DISCOVERY_HINT_DELAY_MS,
      }),
    ).toBe(true);
    expect(
      shouldShowDiscoveryHint({ hostCount: 0, elapsedMs: 10_000 }),
    ).toBe(true);
  });

  it("never shows when at least one host was discovered", () => {
    expect(
      shouldShowDiscoveryHint({ hostCount: 1, elapsedMs: 60_000 }),
    ).toBe(false);
  });

  it("honors a custom delay", () => {
    expect(
      shouldShowDiscoveryHint({
        hostCount: 0,
        elapsedMs: 500,
        delayMs: 500,
      }),
    ).toBe(true);
    expect(
      shouldShowDiscoveryHint({
        hostCount: 0,
        elapsedMs: 499,
        delayMs: 500,
      }),
    ).toBe(false);
  });
});
