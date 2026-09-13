import { expect, it } from "vitest";
import { statusPollingInterval } from "./status-polling";
it("backs off idle and hidden catalog while preserving active recovery cadence", () => {
  expect(statusPollingInterval(0, true)).toBe(10_000);
  expect(statusPollingInterval(0, false)).toBe(30_000);
  expect(statusPollingInterval(1, true)).toBe(2_000);
  expect(statusPollingInterval(1, false)).toBe(2_000);
});
