import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getTranslation } from "@leftcar/ui-tokens";
import { SystemAlertBanners } from "./components/SystemAlertBanners";
import FileShareCard from "./components/FileShareCard";
import SessionPipelineDiagnostics from "./SessionPipelineDiagnostics";
import SessionEncoderDiagnostics from "./SessionEncoderDiagnostics";
import { StopStreamModal } from "./modals/DashboardModals";
import type { SessionRow } from "./sessionTypes";
import QualityOverride from "./QualityOverride";

const t = getTranslation("en");
const session: SessionRow = {
  session: 7,
  sourceName: "Display 1",
  viewerAddr: "192.168.1.2",
  state: "running",
  sourceIndex: 0,
  fps: 60,
  kbps: 0,
  inputRateHz: 0,
  inputEnabled: false,
};
afterEach(() => vi.unstubAllGlobals());

describe("Host user-visible safety states", () => {
  it("announces a pending quality change beside its disabled controls", () => {
    const html = renderToStaticMarkup(<QualityOverride session={session} qualitySupported qualityPercent={40} qualityBusy language="en" onSetQuality={async () => {}} />);
    expect(html).toContain('role="status"');
    expect(html).toContain(t.host.inspector.qualityApplying);
    expect(html).toContain('aria-busy="true"');
  });
  it("does not claim a permission is denied before the initial status reply", () => {
    const props = {
      ready: false,
      error: null,
      inputActionError: null,
      inputPermission: false,
      screenPermission: false,
      platform: "macos" as const,
      inputBusy: null,
      t,
      onRequestPermission() {},
      onOpenAccessibility() {},
      onRefresh() {},
    };
    const html = renderToStaticMarkup(<SystemAlertBanners {...props} />);
    expect(html).not.toContain("Remote Control Permission Required");
    expect(html).not.toContain("Screen Recording Permission Required");
    expect(html).toContain('role="status"');
  });

  it("keeps file sharing locked while its confirmed setting is unknown", () => {
    const html = renderToStaticMarkup(<FileShareCard t={t} />);
    expect(html).toMatch(
      /role="switch"[^>]*disabled|disabled[^>]*role="switch"/,
    );
    expect(html).toContain('role="status"');
  });

  it("renders absent latency telemetry as unknown rather than plausible measurements", () => {
    const html = renderToStaticMarkup(
      <SessionPipelineDiagnostics
        session={session}
        transportLabel="Wi-Fi"
        language="en"
      />,
    );
    expect(html).not.toMatch(/(?:&lt;2|0\.1|0\.2|1\.2|0\.5)ms/);
    expect(html).toContain("Measuring");
  });

  it("does not invent network defaults or zero counters for missing telemetry", () => {
    const html = renderToStaticMarkup(
      <SessionPipelineDiagnostics
        session={session}
        transportLabel="Wi-Fi"
        language="en"
      />,
    );
    expect(html).not.toContain("legacy");
    expect(html).not.toContain("0 sent");
    expect(html).not.toContain("0 failed");
    expect(html).not.toContain("0B");
  });

  it("does not invent a capture backend when no backend has been reported", () => {
    const html = renderToStaticMarkup(
      <SessionEncoderDiagnostics session={session} language="en" />,
    );
    expect(html).not.toContain("ScreenCaptureKit");
  });

  it("shows a failed stop inside the confirmation dialog", () => {
    const props = {
      session,
      busy: false,
      error: "Unable to stop this stream",
      t,
      onCancel() {},
      onConfirm() {},
    };
    const html = renderToStaticMarkup(<StopStreamModal {...props} />);
    expect(html).toContain("Unable to stop this stream");
    expect(html).toContain('role="alert"');
  });
});
