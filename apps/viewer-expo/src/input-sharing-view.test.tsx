import React, { type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DOMParser } from "@xmldom/xmldom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { KmNativeStatus } from "./km-session";
import type { KmIssue, KmFailure } from "./km-session";
import InputSharingScreen from "../app/input-sharing";

const panel = vi.hoisted(() => ({
  language: "en" as "en" | "ko",
  busy: false,
  error: null as KmIssue | null,
  failure: null as KmFailure | null,
  status: { state: "off", reason: null, shizukuRunning: true, shizukuGranted: true, accessibilityEnabled: true,
    displayWidth: 2560, displayHeight: 1600, capabilities: { cursorInjection: false, mouseObservation: false, mouseCapture: false, keyboardCapture: false } } as KmNativeStatus,
  options: { arrangementSide: "right" as "left" | "right", macToAndroid: true, androidToMac: false },
}));
vi.mock("./i18n", async () => {
  const { getTranslation } = await import("@leftcar/ui-tokens");
  return { useAppLanguage: () => ({ t: getTranslation(panel.language) }) };
});
vi.mock("./use-km-model", () => ({ useKmModel: () => ({
  status: panel.status,
  options: panel.options, busy: panel.busy, available: true, error: panel.error, failure: panel.failure, peerHost: "192.168.1.8",
  setOptions() {}, start() {}, stop() {}, refresh() {}, requestShizuku() {}, openAccessibility() {}, cancelStart() {},
}) }));
vi.mock("./theme", () => ({ useAppTheme: () => ({ colors: { textMuted: "currentColor" } }) }));
vi.mock("uniwind", () => ({ withUniwind: (component: unknown) => component }));
vi.mock("react-native-safe-area-context", async () => {
  const { createElement } = await import("react");
  return { SafeAreaView: ({ children }: { children: ReactNode }) => createElement("div", null, children) };
});
// Replace platform renderers only. The screen and real Action/Label/Surface
// primitives still determine labels, selection classes and accessibility state.
vi.mock("react-native", async () => {
  const { createElement } = await import("react");
  const box = ({ children, className }: { children: ReactNode; className?: string }) => createElement("div", { className }, children);
  return {
    ScrollView: box, View: box,
    Text: ({ children, className }: { children: ReactNode; className?: string }) => createElement("span", { className }, children),
    TextInput: () => createElement("input"), ActivityIndicator: () => createElement("span"),
    // SafeArea의 패널 밀도 제공자가 화면 폭을 읽는다(폰 폭 = 배율 1).
    useWindowDimensions: () => ({ width: 390, height: 844, scale: 2, fontScale: 1 }),
    Pressable: ({ children, className, disabled, accessibilityRole, accessibilityState }: {
      children: ReactNode | ((state: { pressed: boolean }) => ReactNode); className?: string; disabled?: boolean;
      accessibilityRole?: string; accessibilityState?: { checked?: boolean };
    }) => createElement("button", { className, disabled, role: accessibilityRole, "aria-checked": accessibilityState?.checked },
      typeof children === "function" ? children({ pressed: false }) : children),
  };
});
function choices(role: "switch" | "radio") {
  const markup = renderToStaticMarkup(<InputSharingScreen />);
  const document = new DOMParser().parseFromString(markup, "text/html");
  return Array.from(document.getElementsByTagName("button"))
    .filter(button => button.getAttribute("role") === role)
    .map(button => ({ text: button.textContent, checked: button.getAttribute("aria-checked"), classes: button.getAttribute("class") }));
}
beforeEach(() => {
  panel.language = "en"; panel.busy = false; panel.error = null; panel.failure = null;
  panel.status.state = "off"; panel.status.reason = null; panel.status.shizukuRunning = true; panel.status.shizukuGranted = true; panel.status.accessibilityEnabled = true;
  panel.options = { arrangementSide: "right", macToAndroid: true, androidToMac: false };
});
describe("visible input-sharing choices", () => {
  it.each([
    ["en", "Input sharing shutdown could not be confirmed. Check the network and system accessibility settings, then retry Stop."],
    ["ko", "입력 공유 종료를 확인하지 못했습니다. 네트워크와 시스템 접근성 설정을 확인하고 중지를 다시 시도하세요."],
  ] as const)("presents one detailed shutdown failure notice in %s", (language, detail) => {
    panel.language = language; panel.status.state = "cleanupFailed"; panel.error = "stopFailed";
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup.split(detail).length - 1).toBe(1);
    expect(markup).toContain(language === "en" ? "Stop input sharing" : "입력 공유 중지");
    expect(markup).not.toContain(language === "en" ? "Start input sharing" : "입력 공유 시작");
  });

  it.each([
    ["en", "connectionSetup", "connection setup", "Connect to the paired Mac"],
    ["en", "androidPreparation", "Android input preparation", "Refresh Android permissions"],
    ["en", "hostApproval", "Host start request", "Check the Mac"],
    ["en", "nativeHandshake", "input connection check", "Stop sharing"],
    ["ko", "connectionSetup", "연결 설정 단계", "페어링된 Mac에 연결"],
    ["ko", "androidPreparation", "Android 입력 준비 단계", "Android 권한을 새로고침"],
    ["ko", "hostApproval", "Host 시작 요청 단계", "Mac의 승인"],
    ["ko", "nativeHandshake", "입력 연결 확인 단계", "공유를 중지"],
  ] as const)("shows a bounded stage and recovery for %s / %s", (language, stage, label, recovery) => {
    panel.language = language; panel.error = "startFailed"; panel.failure = { issue: "startFailed", stage };
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain(label); expect(markup).toContain(recovery);
  });
  it.each(["en", "ko"] as const)("keeps KeyBridge enabled in %s recovery from a native reason", language => {
    panel.language = language;
    panel.status.reason = "KeyBridge 입력 공유 연동 API가 필요합니다. KeyBridge를 활성화한 채 지원 업데이트를 적용하세요";
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain(language === "en" ? "Keep KeyBridge enabled" : "KeyBridge를 켜 둔 채");
    expect(markup).not.toContain("manually disable other");
  });
  it("renders only generic recovery for secret-shaped and embellished native reasons", () => {
    panel.status.reason = "KeyBridge physical key ownership unavailable sessionKey=secret-fixture token=private-fixture keyInput=user-fixture";
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain("Input sharing stopped");
    expect(markup).not.toContain("secret-fixture"); expect(markup).not.toContain("private-fixture"); expect(markup).not.toContain("user-fixture");
    expect(markup).not.toContain("Keep KeyBridge enabled");
  });
  it("prioritizes unconfirmed cleanup over an earlier ordinary startup error", () => {
    panel.error = "keyBridgeUnsupported"; panel.failure = { issue: "keyBridgeUnsupported", stage: "androidPreparation" };
    panel.status.state = "cleanupFailed";
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain("Input sharing shutdown could not be confirmed");
    expect(markup).toContain("manually turn off Leftcar accessibility");
    expect(markup).not.toContain("KeyBridge input ownership integration");
  });
  it.each([
    ["armed", "Sharing on · waiting for an edge crossing"],
    ["macToAndroid", "Mac input controls Android"],
    ["androidToMac", "Android input controls Mac"],
  ] as const)("keeps current native ownership visible for %s", (state, label) => {
    panel.status.state = state;
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain(label);
    expect(markup).toContain("Stop input sharing");
    expect(markup).not.toContain("Start input sharing");
  });
  it("retains permission recovery and retry Stop when cleanup cannot be confirmed", () => {
    panel.error = "stopFailed"; panel.status.shizukuGranted = false; panel.status.accessibilityEnabled = false;
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain("Request Shizuku permission");
    expect(markup).toContain("Open accessibility settings");
    expect(markup).toContain("Stop input sharing");
    expect(markup).not.toContain("Start input sharing");
    expect(markup).toContain("Input sharing shutdown could not be confirmed");
  });
  it("keeps unsolicited native cleanup failure visible after panel remount", () => {
    Object.assign(panel.status, { state: "cleanupFailed", reason: "cleanup unconfirmed" });
    const markup = renderToStaticMarkup(<InputSharingScreen />);
    expect(markup).toContain("Input sharing shutdown could not be confirmed");
    expect(markup).toContain("Stop input sharing");
    expect(markup).not.toContain("Start input sharing");
    expect(markup).not.toContain("Input sharing stopped");
  });
  it("renders approved and unapproved direction values beyond accessibility metadata", () => {
    const rendered = choices("switch");
    expect(rendered.map(({ text, checked }) => ({ text, checked }))).toEqual([
      { text: "Allow Mac → Android · Approved", checked: "true" },
      { text: "Allow Android → Mac · Not approved", checked: "false" },
    ]);
    expect(rendered[0].classes).not.toBe(rendered[1].classes);
  });
  it("moves the visible arrangement marker to the chosen side even when controls are disabled", () => {
    panel.busy = true;
    expect(choices("radio").map(({ text, checked }) => ({ text, checked }))).toEqual([
      { text: "Left", checked: "false" }, { text: "✓ Right", checked: "true" },
    ]);
    panel.options.arrangementSide = "left";
    expect(choices("radio").map(({ text, checked }) => ({ text, checked }))).toEqual([
      { text: "✓ Left", checked: "true" }, { text: "Right", checked: "false" },
    ]);
  });
  it("renders both current approval values in Korean", () => {
    panel.language = "ko";
    panel.options.macToAndroid = false; panel.options.androidToMac = true;
    expect(choices("switch").map(({ text }) => text)).toEqual([
      "Mac → Android 허용 · 허용 안 됨", "Android → Mac 허용 · 허용됨",
    ]);
  });
});
