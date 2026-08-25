import { describe, expect, it } from "vitest";
import { createTerminationNotice, isTerminalSession } from "./streamTermination";

const baseSession = {
  session: 7,
  sourceName: "Built-in Display",
  viewerAddr: "192.168.0.20:5001",
};

describe("stream termination presentation", () => {
  it("keeps running sessions out of the terminal path", () => {
    expect(isTerminalSession({ state: "running" })).toBe(false);
    expect(isTerminalSession({ state: "stopped" })).toBe(true);
    expect(isTerminalSession({ state: "error" })).toBe(true);
  });

  it("explains an operator stop in reader-facing Korean", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "stopped",
      error: "host operator stopped the stream",
    });

    expect(notice.title).toBe("컴퓨터에서 화면 공유를 종료했습니다");
    expect(notice.detail).toContain("연결된 기기에 종료 사실");
    expect(notice.tone).toBe("neutral");
  });

  it("explains the automatic feedback timeout and its threshold", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "error",
      error: "viewer connection lost (feedback timeout)",
    });

    expect(notice.title).toBe("연결된 기기의 응답이 없어 자동 종료했습니다");
    expect(notice.detail).toContain("약 6초");
    expect(notice.tone).toBe("warning");
  });

  it("does not hide an unexpected backend error", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "error",
      error: "encoder failed to start",
    });

    expect(notice.title).toBe("문제가 생겨 화면 공유를 종료했습니다");
    expect(notice.detail).toBe("encoder failed to start");
    expect(notice.tone).toBe("danger");
  });
});
