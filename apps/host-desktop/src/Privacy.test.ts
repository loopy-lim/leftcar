import { describe, expect, it } from "vitest";
import { createToggleGate } from "./toggleGate";

describe("toggle gate", () => {
  it("ignores responses from superseded requests", () => {
    const gate = createToggleGate();
    const first = gate.issue();
    expect(gate.isCurrent(first)).toBe(true);

    // 더 새 토글이 발행되면 앞 요청의 응답·실패는 무시된다.
    const second = gate.issue();
    expect(gate.isCurrent(first)).toBe(false);
    expect(gate.isCurrent(second)).toBe(true);
  });

  it("ignores initial-load responses once a toggle was issued", () => {
    const gate = createToggleGate();
    expect(gate.allowsInitialLoad()).toBe(true);

    gate.issue();
    expect(gate.allowsInitialLoad()).toBe(false);
  });
});
