import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { getTranslation } from "@leftcar/ui-tokens";
import { KmConsentView } from "./KmConsentPanel";
import { saveKmDeviceConsent, type KmDeviceStatus } from "./km-consent";
const status: KmDeviceStatus = { armed: false, owner: "local", epoch: 0, capabilities: { macCapture: false, macInjection: false, android: { cursorInjection: false, mouseObservation: false, mouseCapture: false, keyboardCapture: false } }, blockedReason: "permissions required" };
const t = getTranslation("en");
describe("per-device input sharing approval", () => {
  it.each([
    ["local", "Sharing on · waiting for an edge crossing"],
    ["macToAndroid", "Mac input controls Android"],
    ["androidToMac", "Android input controls Mac"],
  ] as const)("shows current native owner %s without permission recovery for granted Mac authority", (owner, label) => {
    const readyStatus: KmDeviceStatus = { ...status, owner, armed: true, blockedReason: null,
      capabilities: { ...status.capabilities, macCapture: true, macInjection: true } };
    const html = renderToStaticMarkup(<KmConsentView t={t} status={readyStatus} consent={{ macToAndroid: true, androidToMac: true }} busy={false} error={null} onToggle={() => {}} onRefresh={() => {}} onSettings={() => {}} />);
    expect(html).toContain(label);
    expect(html).not.toContain(t.km.openInputMonitoring);
    expect(html).not.toContain(t.km.openMacAccessibility);
    expect(html).not.toContain(t.km.hostBlocked);
  });
  it("keeps independent approvals off and permission recovery visible without screen grants", () => {
    const html = renderToStaticMarkup(<KmConsentView t={t} status={status} consent={{ macToAndroid: false, androidToMac: false }} busy={false} error={null} onToggle={() => {}} onRefresh={() => {}} onSettings={() => {}} />);
    expect(html.match(/aria-checked="false"/g)).toHaveLength(2);
    expect(html).toContain(t.km.bothRequired);
    expect(html).toContain(t.km.openInputMonitoring);
    expect(html).toContain(t.km.openMacAccessibility);
    expect(html).toContain(t.km.hostBlocked);
  });
  it("keeps approvals disabled until current host consent is read", () => {
    const html = renderToStaticMarkup(<KmConsentView t={t} status={null} consent={null} busy={false} error={null} onToggle={() => {}} onRefresh={() => {}} onSettings={() => {}} />);
    expect(html.match(/role="switch"/g)).toHaveLength(2);
    expect(html.match(/disabled=""/g)?.length).toBeGreaterThanOrEqual(2);
    expect(html).toContain(t.km.loading);
    expect(html).not.toContain(t.km.hostBlocked);
  });
  it("revokes a single direction with the exact independent per-device command", async () => {
    const calls: { command: string; args?: unknown }[] = [];
    const confirmed = await saveKmDeviceConsent("device-7", { macToAndroid: false, androidToMac: true }, async <T,>(command: string, args?: unknown) => {
      calls.push({ command, args }); return { macToAndroid: false, androidToMac: true } as T;
    });
    expect(calls).toEqual([{ command: "set_km_consent", args: { deviceId: "device-7", macToAndroid: false, androidToMac: true } }]);
    expect(confirmed).toEqual({ macToAndroid: false, androidToMac: true });
  });
});
