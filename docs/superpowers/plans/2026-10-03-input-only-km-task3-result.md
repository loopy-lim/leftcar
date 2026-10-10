# Task 3 — Android native KM implementation receipt

Implemented in `/private/tmp/leftcar-km-20261003`, Android scope only. No commit, installation, permission grant, device injection, or helper launch was performed by this worker. The root later reported an attached TB522FU Android 16 device and USB-started Shizuku 13.6 running as UID 2000; those device actions were performed by the root, not this worker. This is not an input-runtime acceptance receipt.

## Native interface

`NativeModules.LeftcarKm` exposes `getStatus()`, `prepare()`, `requestShizukuPermission()`, `openAccessibilitySettings()`, `start(config)`, and `stop()`. All return Promises. The event is `leftcarKmStatus`. Status has `state` (`off`, `preparing`, `armed`, `macToAndroid`, `androidToMac`), nullable `reason`, `shizukuRunning`, `shizukuGranted`, `accessibilityEnabled`, physical-pixel `displayWidth` and `displayHeight` from rotation-aware maximum window metrics, and `capabilities: {cursorInjection, mouseObservation, mouseCapture, keyboardCapture}`.

`getStatus` does not bind/create devices. Explicit Start flow must call `prepare`, pass verified capabilities and physical display dimensions to authenticated Host `startKm`, then call native `start`. If negotiation fails, call native `stop`. `prepare` times out after 30 seconds without start. `start` resolves after native transport setup; the authenticated Host State subsequently changes status to Armed. `stop` resolves after serialized cleanup; inspect status reason for cleanup failures.

Start config: `{host, port, sessionId, sessionKey, epoch, arrangementSide: "left"|"right", macToAndroid, androidToMac}`. The seed is base64url, 32 bytes. Epoch uses the JS safe integer range at this boundary. All high-rate input stays on native workers.

## Implemented behavior

- Shell UID 2000 Shizuku UserService, local Binder only, independently resolved caller app UID, app Binder death and one-second heartbeat expiry cleanup. No helper network listener or key logging.
- Real relative UHID mouse and keyboard using UHID_CREATE2/INPUT2, tracked keys/modifiers/buttons, neutral reports and close; fixed keyboard 6KRO and five mouse buttons. Unsupported rollover/buttons stop the session rather than silently dropping state.
- Android 16 hidden observed-motion flag, method presence, setter ordering and service-info readback checks. Armed and Active AndroidToMac both use observe-and-pass for motion; only the selected physical EVIOCGRAB owns mouse consumption. Active keyboard filtering is separate. Touchscreen is excluded. Own `Leftcar KM` devices contribute native cursor feedback only, never outbound physical motion/keys.
- Exact-name, unique physical evdev mouse classification requires REL_X/REL_Y and BTN_MOUSE, rejects touch/ABS/UHID, opens read-only and uses reversible EVIOCGRAB. Closing releases the grab on return, failure, app death and helper expiry. Physical mouse is grabbed only when AndroidToMac activates. No attached Android mouse is required for Mac-origin sharing. Preflight mouseCapture indicates adapter availability; actual node/SELinux access is checked at activation.
- Local held keys/modifiers/buttons defer physical edge activation. KeyBridge lease recognizes `dev.loopy.keybridge`; if enabled with any eligible external physical key device, it requires explicit exported ownership-service acquire ACK. Without such a device, KeyBridge remains enabled and processes incoming UHID once; Leftcar excludes its own UHID from forwarding. Missing/unsupported binding with a physical key device fails OFF. No OS accessibility service is automatically disabled.
- Dedicated foreground connectedDevice service, native encrypted duplex TCP with TCP_NODELAY, bounded queues/frame size, HKDF c2s/s2c keys, ChaCha20-Poly1305 counter framing, 100 ms heartbeat/one-second response timeout, relative motion aggregation every eight milliseconds. No streaming Activity or React event-per-input dependency.
- Host epoch ownership, stale message rejection and queued old-epoch packet purge. Normal return sends new-epoch Heartbeat only after neutral release, physical grab release, and accessibility-mode readback. Host can use it as applied-release confirmation.
- Entry placement is bounded feedback-controlled relative UHID movement using actual observed native cursor positions, not integrated outgoing deltas or an overlay. It fails OFF if native feedback/placement cannot complete within one second or 64 steps. Eight-pixel interior guard avoids immediate edge bounce.

## Verification

Initial missing-implementation tests failed before implementation. Final command used the existing signing identity:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' \
LEFTCAR_INTERNAL_DEBUG_KEYSTORE='/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/debug.keystore' \
./gradlew :app:testDebugUnitTest --tests 'dev.leftcar.viewer.km.*' :app:assembleDebug --console=plain
```

Final result: **BUILD SUCCESSFUL**, 40 KM JVM/Robolectric tests, zero failures/errors. New tests cover HID neutral/modifier/button encoding and rollover, exact shared Rust/Swift crypto vector, authentication/replay rejection, epoch advancement, capture-mode policy, real loopback encrypted duplex TCP, bounded oversized frames, and strict payload validation. Existing KM foundation tests also pass. This is source/build evidence, not lifecycle or physical-input acceptance.

Log: `/private/tmp/leftcar-km-android-build.log`.
APK: `apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk`.
SHA-256: `1842e5ac64506656da8b68a5707af8892b36c2206e8fe3e1cc17059bfc78dbc4`.
APK signature verified with apksigner; signer SHA-256 `dd0f47dc0791450eac89ac2d304930d642ff32ad888a618a1992a6225f3b9655`, matching the installed identity reported by the root. This debug APK predates final Task 4 UI bundling. Parent owns final React Doctor/typecheck/JS gates and release packaging.

## Device-dependent boundaries still requiring acceptance

The hidden Android 16 observer is a gated candidate until verified on the actual OEM build. Capability readback does not prove physical native pointer latency, hover, wheel, keyboard behavior, or out-and-back crossing. EVIOCGRAB permission and pointer freezing, device identity mapping, and simultaneous keyboard ownership must be exercised on hardware. Global source pointer hiding is not claimed: EVIOCGRAB freezes the source pointer by withholding physical movement; it does not hide its last visible image. No fake cursor overlay is used.

The root confirmed installed KeyBridge 0.3.4 lacks the ownership service. KeyBridge must remain enabled. The current no-Android-physical-key-device case can receive Mac UHID input without a lease; general Android physical-key forwarding remains blocked until an acknowledged ownership API is available. Hotplug and source-callback guards revoke unleased sharing before physical forwarding. Android reboot requires Shizuku to be restarted by the already approved USB setup and explicit sharing Start; no auto-arm occurs after authority loss. Rotation stops sharing for recalibration.

## Primary references and attribution

- [Shizuku UserService](https://github.com/RikkaApps/Shizuku-API#userservice) and [setup/reboot behavior](https://shizuku.rikka.app/guide/setup/).
- [Linux UHID protocol](https://docs.kernel.org/hid/uhid.html). The fixed Kotlin HID implementation is independently written; [scrcpy UhidManager](https://github.com/Genymobile/scrcpy/blob/master/server/src/main/java/com/genymobile/scrcpy/control/UhidManager.java), Apache-2.0, was used as a protocol reference, not copied source.
- [Android 16 AccessibilityServiceInfo observed-source API](https://github.com/aosp-mirror/platform_frameworks_base/blob/android16-release/core/java/android/accessibilityservice/AccessibilityServiceInfo.java) and [AccessibilityInputFilter pass/consume behavior](https://github.com/aosp-mirror/platform_frameworks_base/blob/android16-release/services/accessibility/java/com/android/server/accessibility/AccessibilityInputFilter.java).
- [AOSP shell SELinux policy](https://android.googlesource.com/platform/system/sepolicy/+/refs/heads/android16-release/private/shell.te) and [kernel ioctl authorization](https://android.googlesource.com/kernel/common/+/refs/heads/android15-6.6/security/selinux/hooks.c). OEM policy remains a runtime gate.


## Review fix round 1

Pre-fix assigned Kotlin files were copied to `.superpowers/sdd/2026-10-03-input-only-km/task3-before-fix/`. All three Important findings and the deprecated-override warning were addressed; no device write, installation or additional APK package was produced in this round.

1. `KmAccessibilityService.configure` now requests and verifies observed sources for BOTH OBSERVE and CONSUME keyboard modes. The selected physical evdev grab is the only mouse-consuming adapter; unrelated mouse/touchpad callbacks remain OS pass-through. The mode predicate is used by the actual adapter and tested.
2. Host coordination confirmed ReleaseAll(old epoch) → mapped Cursor(old epoch) → State Local(new epoch). The Runtime honors both target X and Y, accepts return placement while AndroidToMac, allows own UHID native feedback in that phase, and retains grab/key ownership until bounded placement finishes. State Local is deferred; periodic heartbeat cannot accidentally ACK the new epoch while placement is pending. Commit order is neutral → ungrab → local service readback → ACK. Missing return placement fails OFF. A Host 500 ms applied-ACK deadline may stop a slow placement before Android's one-second bound; hardware timing is still unverified.
3. Actual runtime transition and disconnect ordering now use `KmOwnerTransition`; actual helper caller authorization, scheduled watchdog, and app Binder-death path use `KmAuthorityLifetime`. JNI-free tests exercise those production seams, including failure to neutralize, failure to restore, no ACK on failures, continuing all disconnect cleanup attempts, unauthorized UID rejection, heartbeat expiry, idempotent Binder death, and bounded real-coordinate placement. This does not substitute for physical Binder/SELinux acceptance.

Additional authorized scope: the root found no external Android input device on the current tablet and required KeyBridge to remain enabled. `KmKeyboardInventory.eligible` is shared by acquisition preflight, device-change invalidation, and the actual key forwarder: external, nonvirtual, not own UHID, key-capable (keyboard/dpad/gamepad/joystick), excluding touchscreen/pen. If no matching device exists, no KeyBridge ownership lease is needed for Mac-origin receive. Any matching device add/change while unleased stops/re-preflights, and every active physical key callback rechecks ownership before forwarding to close the listener race. Tests cover internal/virtual/UHID exclusion, matching physical device, attachment revocation and acknowledged-lease exception. This is limited coexistence for the current Mac-attached set, not general Android physical-keyboard coexistence.

Exact red command (repository root; same signing identity/cache environment):

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' \
LEFTCAR_INTERNAL_DEBUG_KEYSTORE='/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/debug.keystore' \
./apps/viewer-expo/android/gradlew -p apps/viewer-expo/android \
  :app:testDebugUnitTest --tests 'dev.leftcar.viewer.km.KmLifecycleTest' \
  -x buildViewerNative -x copyViewerSo --console=plain
```

`/private/tmp/leftcar-km-fix-red.log`: BUILD FAILED in 2s; unresolved production lifecycle/placement seam symbols. The same command with the added inventory regression failed before implementation: `/private/tmp/leftcar-km-inventory-red.log`, BUILD FAILED in 2s, unresolved `KmKeyboardInventory`.

Exact final green command:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' \
LEFTCAR_INTERNAL_DEBUG_KEYSTORE='/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/debug.keystore' \
./apps/viewer-expo/android/gradlew -p apps/viewer-expo/android \
  :app:testDebugUnitTest --tests 'dev.leftcar.viewer.km.KmLifecycleTest' \
  --tests 'dev.leftcar.viewer.km.KmNativeSessionTest' \
  --tests 'dev.leftcar.viewer.km.KmDuplexTest' \
  -x buildViewerNative -x copyViewerSo --console=plain
```

`/private/tmp/leftcar-km-fix-green.log`: **BUILD SUCCESSFUL in 8s**, 17 focused tests (7 lifecycle/inventory + 7 HID/crypto/epoch + 3 duplex), zero failures/errors. No Kotlin warning/error lines, including the previously reported deprecated override. Gradle's existing generic Gradle 10 deprecation notice remains. Final APK packaging belongs to the root after Task 4 UI integration; the earlier APK/hash above predates these fixes.
