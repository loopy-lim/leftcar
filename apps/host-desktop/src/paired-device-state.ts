import type { SourceGrantView } from "./SourceGrantEditor";

export interface PairedDevice {
  source_grants: SourceGrantView;
  device_id: string;
  name: string;
  paired_at: string;
}
export interface PairedDeviceState {
  revision: number;
  devices: PairedDevice[];
}
export interface RevokeOutcome {
  removedDevices: { deviceId: string; credentialId: string }[];
  stateRevision: number;
  persistenceErrors: string[];
}

// One webview belongs to one Host process/profile. Only complete snapshots
// advance revision. A partial reply proves facts about its own credential only.
let state: PairedDeviceState & { administrativeError: string | null } = {
  revision: -1,
  devices: [],
  administrativeError: null,
};
const uncertain = new Map<string, { epoch: number; error: string }>();
let failureEpoch = 0;
let administrativeRevision = -1;
// Keep removals until a complete snapshot acknowledges membership. They cannot
// be pruned by another device's partial reply. New complete snapshots bound
// this map; old snapshots are then rejected by the complete-snapshot revision.
const removedCredentials = new Map<string, number>();
const listeners = new Set<() => void>();
function publish(next: typeof state): void {
  state = next;
  for (const listener of listeners) listener();
}

export function getPairedDeviceState(): typeof state {
  return state;
}

export function subscribePairedDeviceState(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function sourceGrantUncertaintyEpoch(credentialId: string): number {
  return uncertain.get(credentialId)?.epoch ?? 0;
}

function retainUncertainty(grants: SourceGrantView): SourceGrantView {
  const failure = uncertain.get(grants.credentialId);
  return failure ? { ...grants, reviewRequired: true, persistenceError: failure.error } : grants;
}
export function receivePairedDeviceState(snapshot: PairedDeviceState): void {
  if (snapshot.revision < state.revision) return;
  const previousById = new Map(state.devices.map(device => [device.device_id, device]));
  const devices = snapshot.devices
    .filter(device => !removedCredentials.has(device.source_grants.credentialId))
    .map(device => {
      const previous = previousById.get(device.device_id);
      previousById.delete(device.device_id);
      // A known later partial confirmation also proves that credential existed
      // after this historical snapshot. Do not erase it with older membership.
      if (previous && previous.source_grants.stateRevision > snapshot.revision) {
        return previous;
      }
      const sameCredential = previous?.source_grants.credentialId === device.source_grants.credentialId;
      const grants = sameCredential && previous.source_grants.revision > device.source_grants.revision
        ? previous.source_grants
        : device.source_grants;
      return { ...device, source_grants: retainUncertainty(grants) };
    });
  for (const previous of previousById.values()) {
    if (previous.source_grants.stateRevision > snapshot.revision) devices.push(previous);
  }
  const current = new Set(devices.map(device => device.source_grants.credentialId));
  for (const credential of uncertain.keys()) {
    if (!current.has(credential)) uncertain.delete(credential);
  }
  const membership = new Set(snapshot.devices.map(device => device.source_grants.credentialId));
  for (const [credential, revision] of removedCredentials) {
    if (snapshot.revision >= revision && !membership.has(credential)) {
      removedCredentials.delete(credential);
    }
  }
  publish({ ...state, ...snapshot, devices });
}
export function confirmSourceGrants(
  deviceId: string,
  grants: SourceGrantView,
  startedAtEpoch?: number,
): void {
  const current = state.devices.find(device => device.device_id === deviceId);
  if (!current || current.source_grants.credentialId !== grants.credentialId ||
      current.source_grants.revision > grants.revision) {
    return;
  }
  // A rejected command has no causally bound Host revision. Only a save issued
  // after the latest observed failure can resolve that exact uncertainty.
  if (uncertain.get(grants.credentialId)?.epoch === startedAtEpoch) {
    uncertain.delete(grants.credentialId);
  }
  const confirmed = retainUncertainty({
    ...grants,
    stateRevision: Math.max(grants.stateRevision, current.source_grants.stateRevision),
  });
  publish({
    ...state,
    devices: state.devices.map(device => device === current
      ? { ...device, source_grants: confirmed }
      : device),
  });
}
export function markSourceGrantsUncertain(deviceId: string, credentialId: string, error: string): void {
  const current = state.devices.find(device =>
    device.device_id === deviceId && device.source_grants.credentialId === credentialId);
  if (!current) return;
  uncertain.set(credentialId, { epoch: ++failureEpoch, error });
  publish({
    ...state,
    devices: state.devices.map(device => device === current
      ? { ...device, source_grants: retainUncertainty(device.source_grants) }
      : device),
  });
}
export function receiveRevokeOutcome(outcome: RevokeOutcome): void {
  // A full snapshot can supersede membership; an unrelated partial reply cannot.
  if (outcome.stateRevision > state.revision) {
    for (const removed of outcome.removedDevices) {
      removedCredentials.set(removed.credentialId, Math.max(
        outcome.stateRevision,
        removedCredentials.get(removed.credentialId) ?? -1,
      ));
      uncertain.delete(removed.credentialId);
    }
  }
  const devices = state.devices.filter(device => !removedCredentials.has(device.source_grants.credentialId));
  const error = outcome.persistenceErrors.join("\n");
  const administrativeError = error || (outcome.stateRevision >= administrativeRevision ? null : state.administrativeError);
  administrativeRevision = Math.max(administrativeRevision, outcome.stateRevision);
  publish({ ...state, devices, administrativeError });
}
