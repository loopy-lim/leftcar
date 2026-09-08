import { describe, expect, it } from "vitest";
import {
  markConnected,
  markPairingStale,
  markUserDisconnected,
  noteAutoReconnectAttempt,
  shouldAutoReconnect,
  shouldAutoReconnectFromGate,
} from "./auto-reconnect";

function decision(overrides: Partial<Parameters<typeof shouldAutoReconnect>[0]> = {}) {
  return {
    hasClient: false,
    hasRecentHost: true,
    userDisconnected: false,
    pairingStale: false,
    lastAttemptAt: null,
    now: 100_000,
    ...overrides,
  };
}

describe("shouldAutoReconnect", () => {
  it("attempts when disconnected with a recent host", () => {
    expect(shouldAutoReconnect(decision())).toBe(true);
  });

  it("never attempts while a client exists or no recent host is known", () => {
    expect(shouldAutoReconnect(decision({ hasClient: true }))).toBe(false);
    expect(shouldAutoReconnect(decision({ hasRecentHost: false }))).toBe(false);
  });

  it("respects the user's explicit disconnect and a stale pairing", () => {
    expect(shouldAutoReconnect(decision({ userDisconnected: true }))).toBe(false);
    expect(shouldAutoReconnect(decision({ pairingStale: true }))).toBe(false);
  });

  it("throttles repeated attempts inside the minimum interval", () => {
    expect(
      shouldAutoReconnect(decision({ lastAttemptAt: 95_000, now: 100_000 })),
    ).toBe(false);
    expect(
      shouldAutoReconnect(decision({ lastAttemptAt: 89_999, now: 100_000 })),
    ).toBe(true);
    expect(
      shouldAutoReconnect(
        decision({ lastAttemptAt: 90_000, now: 100_000, minIntervalMs: 1_000 }),
      ),
    ).toBe(true);
  });
});

describe("auto-reconnect gate", () => {
  it("blocks after explicit disconnect and reopens after any successful connect", () => {
    markUserDisconnected();
    expect(shouldAutoReconnectFromGate(false, true, 200_000)).toBe(false);
    markConnected();
    expect(shouldAutoReconnectFromGate(false, true, 200_000)).toBe(true);
  });

  it("blocks after a silent 401 until a successful connect resets it", () => {
    markPairingStale();
    expect(shouldAutoReconnectFromGate(false, true, 200_000)).toBe(false);
    markConnected();
    expect(shouldAutoReconnectFromGate(false, true, 200_000)).toBe(true);
  });

  it("applies the attempt throttle through the gate snapshot", () => {
    markConnected();
    noteAutoReconnectAttempt(300_000);
    expect(shouldAutoReconnectFromGate(false, true, 305_000)).toBe(false);
    expect(shouldAutoReconnectFromGate(false, true, 311_000)).toBe(true);
  });
});
