# Full KM implementation status: BLOCKED

The existing foundation does not provide bidirectional physical KM. The connected device is Lenovo TB522FU (Android 16/API 36). Existing accessibility mouse observation does not confer Android OS cursor/hover injection authority; `dispatchGesture` is touch, not a mouse injector. App UID cannot access `/dev/uhid`, and `INJECT_EVENTS` is privileged. The existing KeyBridge grant belongs to a separately signed application and cannot be assumed transferable. No privileged helper, root, ADB product transport, permission auto-approval, or fake pointer route was introduced.

## Architecture decision

Keep KM ownership and authorization separate from streaming; a real Android cursor actuator must be a separately authorized adapter. Alternative A: user-approved OEM/platform authority preserving direct Wi-Fi product transport. Alternative B: separately approved USB-started shell UHID helper, which adds a debugging startup dependency and requires an explicit security/lifecycle policy. Neither route is currently approved; full KM remains blocked. See `docs/superpowers/plans/2026-10-03-wifi-km-end-to-end-route.json` for device findings and the staged implementation plan.

## Work and verification

Added `tools/wifi-km-e2e-check.py`, a standard-library recorded-evidence validator requiring both directions, mouse, keyboard, real cursor and hover, two edge crossings, and safety observations. `python3 tools/wifi-km-e2e-check.py --help` and `--self-test` pass; nonexistent evidence fails closed. This is an acceptance tool, not an implementation or evidence that hardware worked. No APK was built or installed; there is no newly approved actuator to deploy. Existing installed signature/data were not modified. Rust, Android, JS, and React gates have not been verified by this work; do not infer full completion.

## Attention required

Choose and authorize an actual TB522FU Android OS mouse injector: (A) provide an OEM/platform-managed component and its API/installation authority, or (B) explicitly approve the shell UHID helper, exact privilege scope, USB startup/debug policy, revocation and restart behavior. Human OS permissions for Mac and Android remain separate. Until then, neither a full-KM success claim nor a gesture-only substitute is acceptable.

Reproduce checker: `python3 tools/wifi-km-e2e-check.py --help && python3 tools/wifi-km-e2e-check.py --self-test`; later `python3 tools/wifi-km-e2e-check.py --evidence /path/to/physical-record.json`.
