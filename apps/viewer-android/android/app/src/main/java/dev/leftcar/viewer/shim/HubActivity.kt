package dev.leftcar.viewer.shim

import android.app.Activity
import android.os.Bundle

/**
 * Hub window (docs/03 §3.2): pairing, source catalog, window management UI.
 *
 * SHIM BOUNDARY (docs/09 §9): this file may declare Activities and host the
 * React Native surface. It must NOT hold session state, network loops, codec
 * policy, or business models — those live in the Rust core via Rustra.
 */
class HubActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // RN host attachment lands with H05 (device phase). The shim only
        // owns the component registration, never app logic.
    }
}
