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

  it("presents an exact viewer close as a normal device-initiated stop", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "stopped",
      error: "viewer closed stream",
    });

    expect(notice.title).toBe("연결된 기기에서 화면 공유를 종료했습니다");
    expect(notice.detail).toContain("뒤로 가기");
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

  it("leads with a friendly message but keeps an unexpected backend error visible", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "error",
      error: "encoder failed to start",
    });

    expect(notice.title).toBe("문제가 생겨 화면 공유를 종료했습니다");
    expect(notice.detail).toBe(
      "화면을 가져오거나 보내는 중 문제가 발생했습니다. (encoder failed to start)",
    );
    expect(notice.tone).toBe("danger");
  });

  it("maps screen-recording permission failures to a settings guide", () => {
    const notice = createTerminationNotice({
      ...baseSession,
      state: "error",
      error: "startCapture failed: screen-recording permission required",
    });

    expect(notice.title).toBe("화면 공유 권한 문제로 종료했습니다");
    expect(notice.detail).toContain("화면 녹화 권한");
    expect(notice.tone).toBe("danger");
  });
});
