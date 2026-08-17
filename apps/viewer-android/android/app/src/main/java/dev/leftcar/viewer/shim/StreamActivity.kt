package dev.leftcar.viewer.shim

import android.app.Activity
import android.os.Bundle

/**
 * Stream window (docs/03 §3.2): one remote source per task.
 *
 * Manifest: launchMode=standard + documentLaunchMode=always, so each open is
 * a new task and Android XR Home Space shows it as an independent window
 * (docs/02 §2.2). No pairing secrets in Intent extras — only the opaque
 * launch handle; the Rust core re-validates authorization (docs/02 §2.2 주의).
 */
class StreamActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // 1. read opaque launch handle from intent data URI
        // 2. bind SessionService
        // 3. token -> source lease (Rust core)
        // 4. hand Surface to leftcar_stream_attach_surface
        // All policy stays in Rust; this shim only forwards lifecycle.
    }
}
