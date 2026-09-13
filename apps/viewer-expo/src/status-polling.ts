/** Active stream recovery stays prompt even when the catalog is hidden. */
export function statusPollingInterval(activeStreams: number, visible: boolean): number {
  return activeStreams > 0 ? 2_000 : visible ? 10_000 : 30_000;
}
