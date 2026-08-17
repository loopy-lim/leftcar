/**
 * HubApp (H04): paired hosts, source catalog, launch stream windows.
 * Rendered by HubActivity. Pure presentational + command wiring; the real
 * Rustra client is injected so tests run without React Native.
 */
import React from "react";
import type { StreamWindowLauncherSpec } from "../specs/StreamWindowLauncherSpec";

export interface SourceRow {
  sourceId: string;
  displayName: string;
  kind: "window" | "display";
  approved: boolean;
  available: boolean;
}

export interface HubCommands {
  listRemoteSources(): Promise<SourceRow[]>;
  createStreamLaunch(sourceId: string): Promise<string>;
}

export interface HubState {
  sources: SourceRow[];
  launching: string | null;
  error: string | null;
}

/** Choose what open does for a source (same-source policy, docs/05 §5.3). */
export function decideOpenAction(
  source: SourceRow,
  openSources: Set<string>,
): { action: "launch" | "focus" | "disabled"; reason?: string } {
  if (!source.approved) return { action: "disabled", reason: "not approved" };
  if (!source.available) return { action: "disabled", reason: "unavailable" };
  if (openSources.has(source.sourceId)) return { action: "focus" };
  return { action: "launch" };
}

export function HubApp(_: { __unused?: never }) {
  // Presentational shell; actual wiring lands with the RN host (H05 device
  // phase). Kept as a typed placeholder so Codegen has a stable target.
  return null;
}

export async function openSourceWindow(
  commands: HubCommands,
  launcher: StreamWindowLauncherSpec,
  source: SourceRow,
  openSources: Set<string>,
): Promise<string | null> {
  const decision = decideOpenAction(source, openSources);
  if (decision.action === "disabled") return null;
  if (decision.action === "focus") {
    await launcher.focus(source.sourceId);
    return null;
  }
  const handle = await commands.createStreamLaunch(source.sourceId);
  const instanceId = `instance-${Date.now().toString(36)}`;
  const doc = await launcher.open(handle, source.sourceId, instanceId);
  return doc;
}
