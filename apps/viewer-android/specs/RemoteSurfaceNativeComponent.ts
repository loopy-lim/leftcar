/**
 * RemoteSurface Fabric component spec (docs/02 §7.2).
 *
 * Owns a SurfaceView whose Surface jobject is handed to Rust via the C ABI
 * (leftcar_stream_attach_surface). No frame bytes cross this boundary.
 */
export interface RemoteSurfaceNativeComponentProps {
  /** StreamInstanceId this surface renders. */
  instanceId: string;
  /** Opaque native surface handle (from the Kotlin shim), 0 when absent. */
  surfaceHandle: number;
}
