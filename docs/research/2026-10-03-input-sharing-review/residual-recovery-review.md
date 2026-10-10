# Residual helper-death recovery review

**Dead-helper retention finding: ADDRESSED.**

Reviewed only `residual-recovery.diff`, its report/manifest, and the preceding scoped finding. All three current file hashes match the manifest. No tests rerun; no source, device, permission or Git changes; no subagents.

The runtime now records the exact connected authority Binder and checks identity on the worker before acting on death. Death of a retained helper remains relevant after generation advancement; stale retired-Binder deaths cannot stop its replacement. Cleanup exceptions alone do not prove death: only that exact Binder's death notification or confirmed non-liveness can substitute for helper RPC cleanup. A failed liveness query remains unconfirmed. Local accessibility restoration must also succeed before OFF; a live failing helper or failed local restoration preserves `cleanupFailed` and retry. Successful retirement clears authority/binding state, and only an explicit subsequent prepare creates a new unarmed generation.

The new production-runtime regressions exercise permanent dead proxies, failed-then-successful local restoration, death after failed Stop, stale old death/connection callbacks, and same-proxy live cleanup recovery. The supplied 10 passing native tests are accepted as reported evidence, not independently rerun.

**New Critical findings: 0. New Important findings: 0. Scoped residual verdict: APPROVED.**

This closes the residual source-level recovery defect and leaves no open Critical/Important finding from this review chain. Existing physical/OEM acceptance boundaries remain: these JVM seams do not establish actual Binder delivery, process/FD termination timing, accessibility restoration or physical input recovery. Root's packaging, installation and runtime checks remain separate.
