/**
 * Host desktop shell UI model (H15 placeholder scope).
 * The Tauri shell wires these to the generated Rustra host client.
 */

export interface HostSnapshotView {
  hostId: string;
  platform: "macos" | "windows" | "linux";
  pairingState:
    | "unpaired" | "advertising" | "awaiting_host_approval"
    | "paired_offline" | "connecting" | "connected" | "revoked";
  pairedDevices: Array<{ deviceId: string; displayName: string }>;
  approvedSources: Array<{ sourceId: string; displayName: string }>;
  activeStreamCount: number;
}

/** Menu-bar status line (docs/07 §10: capture visibility). */
export function trayStatus(snapshot: HostSnapshotView): string {
  if (snapshot.activeStreamCount > 0) {
    return `Leftcar — 화면 전송 중 (${snapshot.activeStreamCount})`;
  }
  if (snapshot.pairingState === "connected") return "Leftcar — 연결됨, 대기 중";
  return "Leftcar";
}

/** Stop-all must be always available while capturing (docs/07 §10). */
export function canStopAll(snapshot: HostSnapshotView): boolean {
  return snapshot.activeStreamCount > 0;
}
