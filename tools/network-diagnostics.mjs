import { spawnSync } from 'node:child_process';
import { isIP } from 'node:net';

export const STATUS_TIMEOUT_MS = 5_000;
export const STATUS_OUTPUT_LIMIT = 4 * 1024 * 1024;

// Schema and route precedence: tailscale/ipn/ipnstate/ipnstate.go and
// tailscale/cmd/tailscale/cli/status.go. Active is recent traffic, not proof
// that Leftcar can connect. Never expose the raw inventory or error output.
const BACKEND_STATES = new Set(['NoState', 'NeedsLogin', 'NeedsMachineAuth', 'Stopped', 'Starting', 'Running']);
const HELP = 'Usage: bun tools/network-diagnostics.mjs --peer <tailnet-IP-or-full-MagicDNS-name>\nReads bounded local Tailscale status only; does not ping, connect, or change settings.';

export function normalizePeer(value) {
  if (typeof value !== 'string' || !value || value.length > 253 || value !== value.trim()) {
    throw new Error('Provide one peer IP or full MagicDNS name; ports and URLs are not accepted.');
  }
  if (isIP(value) === 4) return value;
  if (isIP(value) === 6 && !value.includes('%')) return new URL(`http://[${value}]/`).hostname.slice(1, -1);
  const hostname = value.toLowerCase().replace(/\.$/, '');
  if (!hostname.includes('.') || !hostname.split('.').every(label =>
    label.length >= 1 && label.length <= 63 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(label))) {
    throw new Error('Provide one peer IP or full MagicDNS name; ports and URLs are not accepted.');
  }
  return hostname;
}

function normalizedOrNull(value) {
  try { return normalizePeer(value); } catch { return null; }
}

function matchesPeer(peer, target) {
  return peer && typeof peer === 'object' && !Array.isArray(peer) && (
    normalizedOrNull(peer.DNSName) === target ||
    (Array.isArray(peer.TailscaleIPs) && peer.TailscaleIPs.some(ip => normalizedOrNull(ip) === target))
  );
}

function report(code, extra = {}) {
  return {
    schema: 1,
    source: 'tailscale-status',
    code,
    backendState: 'unknown',
    peerFound: false,
    peerOnline: null,
    recentlyActive: null,
    recentRoute: 'unknown',
    routeEvidence: 'none',
    streamReachability: 'unverified',
    ...extra,
  };
}

export function summarizeStatus(status, peer) {
  const target = normalizePeer(peer);
  if (!status || typeof status !== 'object' || Array.isArray(status)) return report('INVALID_STATUS');
  const backendState = BACKEND_STATES.has(status.BackendState) ? status.BackendState : 'unknown';
  if (backendState !== 'Running') return report('BACKEND_NOT_RUNNING', { backendState });
  if (matchesPeer(status.Self, target)) return report('SELF_TARGET', { backendState });
  if (!status.Peer || typeof status.Peer !== 'object' || Array.isArray(status.Peer)) {
    return report('PEER_STATUS_UNAVAILABLE', { backendState });
  }
  const peers = Object.values(status.Peer).filter(candidate => matchesPeer(candidate, target));
  if (peers.length !== 1) return report(peers.length ? 'AMBIGUOUS_PEER' : 'PEER_NOT_FOUND', { backendState });
  const selected = peers[0];
  const peerOnline = typeof selected.Online === 'boolean' ? selected.Online : null;
  const recentlyActive = typeof selected.Active === 'boolean' ? selected.Active : null;
  const details = { backendState, peerFound: true, peerOnline, recentlyActive };
  if (selected.Expired === true) return report('PEER_KEY_EXPIRED', details);
  if (peerOnline !== true) return report(peerOnline === false ? 'PEER_OFFLINE' : 'PEER_STATE_UNKNOWN', details);
  if (recentlyActive !== true) return report('NO_RECENT_TRAFFIC', details);
  if (typeof selected.CurAddr !== 'string' || typeof selected.Relay !== 'string' ||
    (selected.PeerRelay !== undefined && typeof selected.PeerRelay !== 'string')) {
    return report('ROUTE_UNKNOWN', details);
  }
  const present = value => typeof value === 'string' && value.length > 0;
  // Match the official CLI: CurAddr wins; Relay alone is not evidence of a
  // DERP route when there is no recent activity. Do not return either address.
  const recentRoute = present(selected.CurAddr) ? 'direct'
    : present(selected.PeerRelay) ? 'peer-relay'
      : present(selected.Relay) ? 'derp' : 'unknown';
  return report(recentRoute === 'unknown' ? 'ROUTE_UNKNOWN' : 'RECENT_ROUTE_OBSERVED', {
    ...details,
    recentRoute,
    routeEvidence: recentRoute === 'unknown' ? 'none' : 'daemon-recent-state',
  });
}

export function diagnosePeer(peer, { run = spawnSync } = {}) {
  normalizePeer(peer); // Reject invalid input before running a process.
  let result;
  try {
    result = run('tailscale', ['status', '--json', '--peers=true'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: STATUS_TIMEOUT_MS,
      maxBuffer: STATUS_OUTPUT_LIMIT,
      killSignal: 'SIGKILL',
      windowsHide: true,
    });
  } catch {
    return report('STATUS_FAILED');
  }
  if (result?.error || result?.signal || result?.status !== 0) {
    const codes = { ENOENT: 'TAILSCALE_UNAVAILABLE', ETIMEDOUT: 'STATUS_TIMEOUT', ENOBUFS: 'STATUS_TOO_LARGE' };
    return report(codes[result?.error?.code] ?? 'STATUS_FAILED');
  }
  if (typeof result.stdout !== 'string' || Buffer.byteLength(result.stdout) > STATUS_OUTPUT_LIMIT) {
    return report('STATUS_TOO_LARGE');
  }
  try { return summarizeStatus(JSON.parse(result.stdout), peer); }
  catch { return report('INVALID_STATUS'); }
}

export function main(args) {
  if (args.length === 1 && (args[0] === '--help' || args[0] === '-h')) {
    console.log(HELP);
    return 0;
  }
  if (args.length !== 2 || args[0] !== '--peer') {
    console.error(HELP);
    return 2;
  }
  try {
    const result = diagnosePeer(args[1]);
    console.log(JSON.stringify(result, null, 2));
    // This exit code means local status acquisition, not stream acceptance.
    return ['TAILSCALE_UNAVAILABLE', 'STATUS_FAILED', 'STATUS_TIMEOUT', 'STATUS_TOO_LARGE', 'INVALID_STATUS'].includes(result.code) ? 1 : 0;
  } catch {
    console.error('Invalid peer. Use one IP or full MagicDNS name without a port or URL.');
    return 2;
  }
}

if (import.meta.main) process.exitCode = main(process.argv.slice(2));
