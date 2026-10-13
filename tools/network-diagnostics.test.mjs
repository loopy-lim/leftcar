import { expect, test, vi } from 'vitest';
import { diagnosePeer, normalizePeer, STATUS_OUTPUT_LIMIT, STATUS_TIMEOUT_MS, summarizeStatus } from './network-diagnostics.mjs';

const address = '100.64.0.9';
const selected = overrides => ({
  TailscaleIPs: [address, 'fd7a:115c:a1e0::9'], DNSName: 'selected.tail-example.ts.net.',
  Online: true, Active: true, CurAddr: '', PeerRelay: '', Relay: 'region-private', ...overrides,
});
const status = overrides => ({
  BackendState: 'Running',
  AuthURL: 'private-login-url', User: { 1: { LoginName: 'private@example.com' } },
  Peer: { 'nodekey:private-selected': selected(overrides), 'nodekey:private-other': {
    DNSName: 'other.private.ts.net.', TailscaleIPs: ['100.64.0.8'], Online: true, Active: true,
    CurAddr: '198.51.100.8:1234',
  } },
});

test.each([
  [{ CurAddr: '198.51.100.9:1234', PeerRelay: '100.64.0.7:1234:9' }, 'direct'],
  [{ PeerRelay: '100.64.0.7:1234:9' }, 'peer-relay'],
  [{}, 'derp'],
  [{ Relay: '' }, 'unknown'],
])('reports only the selected recently active route %j', (overrides, route) => {
  const result = summarizeStatus(status(overrides), address);
  expect(result.recentRoute).toBe(route);
  expect(result.streamReachability).toBe('unverified');
  expect(result.peerFound).toBe(true);
  expect(result.peerOnline).toBe(true);
});

test.each([
  [{ Active: false, CurAddr: '198.51.100.9:1234' }, 'NO_RECENT_TRAFFIC'],
  [{ Active: undefined }, 'NO_RECENT_TRAFFIC'],
  [{ Online: false }, 'PEER_OFFLINE'],
  [{ Online: undefined }, 'PEER_STATE_UNKNOWN'],
  [{ Expired: true }, 'PEER_KEY_EXPIRED'],
])('does not promote stored route hints to a usable connection %j', (overrides, code) => {
  expect(summarizeStatus(status(overrides), address)).toMatchObject({ code, recentRoute: 'unknown' });
});

test('matches exact full DNS and canonical IPv6 without hostname guessing', () => {
  expect(summarizeStatus(status(), 'SELECTED.TAIL-EXAMPLE.TS.NET').peerFound).toBe(true);
  expect(summarizeStatus(status(), 'fd7a:115c:a1e0:0:0:0:0:9').peerFound).toBe(true);
  expect(summarizeStatus(status(), 'other.tail-example.ts.net').code).toBe('PEER_NOT_FOUND');
  expect(summarizeStatus({ ...status(), Self: selected() }, address).code).toBe('SELF_TARGET');
  const duplicate = status(); duplicate.Peer.extra = selected();
  expect(summarizeStatus(duplicate, address).code).toBe('AMBIGUOUS_PEER');
});

test('never emits inventory, identities, remote addresses, login URLs or arbitrary state strings', () => {
  const result = summarizeStatus(status({ CurAddr: '198.51.100.9:1234' }), address);
  const serialized = JSON.stringify(result);
  for (const secret of ['100.64', '198.51', 'private', 'nodekey', '@', 'ts.net']) expect(serialized).not.toContain(secret);
  expect(summarizeStatus({ ...status(), BackendState: 'secret-login-url' }, address).backendState).toBe('unknown');
});

test('rejects malformed status and unavailable peer schemas conservatively', () => {
  for (const input of [null, [], 'private-text', 4]) expect(summarizeStatus(input, address).code).toBe('INVALID_STATUS');
  for (const Peer of [null, [], 'private-text']) expect(summarizeStatus({ BackendState: 'Running', Peer }, address).code).toBe('PEER_STATUS_UNAVAILABLE');
  expect(summarizeStatus({ BackendState: 'Stopped' }, address).code).toBe('BACKEND_NOT_RUNNING');
  for (const fields of [{ CurAddr: undefined }, { CurAddr: true }, { PeerRelay: 1 }, { Relay: null }]) {
    expect(summarizeStatus(status(fields), address)).toMatchObject({ code: 'ROUTE_UNKNOWN', recentRoute: 'unknown' });
  }
});

test('reads bounded local status without passing a peer to a command or probing the network', () => {
  const run = vi.fn(() => ({ status: 0, stdout: JSON.stringify(status()), stderr: 'private-stderr' }));
  expect(diagnosePeer(address, { run }).recentRoute).toBe('derp');
  expect(run).toHaveBeenCalledWith('tailscale', ['status', '--json', '--peers=true'], expect.objectContaining({
    timeout: STATUS_TIMEOUT_MS, maxBuffer: STATUS_OUTPUT_LIMIT, killSignal: 'SIGKILL', stdio: ['ignore', 'pipe', 'pipe'],
  }));
});

test.each([
  [{ error: { code: 'ENOENT' }, status: null }, 'TAILSCALE_UNAVAILABLE'],
  [{ error: { code: 'ETIMEDOUT' }, status: null }, 'STATUS_TIMEOUT'],
  [{ error: { code: 'ENOBUFS' }, status: null }, 'STATUS_TOO_LARGE'],
  [{ status: 1, stdout: 'private-secret', stderr: 'private-secret' }, 'STATUS_FAILED'],
  [{ status: 0, stdout: 'private-secret' }, 'INVALID_STATUS'],
  [{ status: 0, stdout: 'x'.repeat(STATUS_OUTPUT_LIMIT + 1) }, 'STATUS_TOO_LARGE'],
])('contains acquisition failure case %# without raw output', (output, code) => {
  const result = diagnosePeer(address, { run: () => output });
  expect(result.code).toBe(code);
  expect(JSON.stringify(result)).not.toContain('private-secret');
});

test('invalid caller input is rejected before status acquisition', () => {
  const run = vi.fn();
  for (const peer of ['', 'selected', 'https://host.ts.net', '100.64.0.9:7777', '--help', 'host.ts.net\n', 'fe80::1%en0', '*']) {
    expect(() => diagnosePeer(peer, { run })).toThrow();
  }
  expect(run).not.toHaveBeenCalled();
  expect(normalizePeer('SELECTED.TAIL-EXAMPLE.TS.NET.')).toBe('selected.tail-example.ts.net');
});
