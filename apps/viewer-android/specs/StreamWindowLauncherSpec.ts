/**
 * StreamWindowLauncher TurboModule spec (docs/02 §9).
 *
 * Opens a new document task for a stream window. Source of truth for RN
 * Codegen; the Kotlin implementation is a thin shim that builds the Intent
 * and passes the opaque launch handle through initial props.
 */
export interface StreamWindowLauncherSpec {
  /**
   * Launch a StreamActivity as a new document task.
   * Returns the task's document URI for identity bookkeeping.
   */
  open(launchHandle: string, sourceId: string, instanceId: string): Promise<string>;
  /** Focus the existing window for a source (same-source policy). */
  focus(sourceId: string): Promise<boolean>;
  /** Close the task for an instance (release lease). */
  close(instanceId: string): Promise<void>;
}
