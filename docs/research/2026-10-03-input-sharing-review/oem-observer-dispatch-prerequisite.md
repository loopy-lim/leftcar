# OEM own-service dispatch prerequisite — 2026-10-04 KST

## Result

The exact deployed AccessibilityService dispatch implementation independently gates motion callbacks on the owning app instance's local `mMotionEventSources`. Applying service info through a shell-held exact own-service connection does not by itself update that local field. For this own service's no-IME configuration, the inspected code establishes **one local motion-dispatch field** that must remain synchronized with the applied server motion mask. This is a code prerequisite, not proof that the proposed in-app hidden-member access works or that runtime delegation/physical passthrough is accepted.

Only the previously pulled and host/device hash-matched `/private/tmp/leftcar-km-oem-observer-framework/framework.jar` was read. SHA-256 `1b384aa5dd38e70b35ff00504ee4dcf1a51593545a09629cbd33fd523b496065`, bytes 48528604. No new device read, jar pull, tool/download, Java probe, app/helper invocation, own-service member invocation, connection acquisition/call, settings, input, UI, grant, restart, source edit, or retry was performed. Method-only apkanalyzer reads were made for the exact prerequisites; previous setter/send/cache extracts were reused.

## Actual local callback gate

`sendMotionEventToCallback(MotionEvent)` is package-access/blacklist in the DEX. It:

1. Optionally routes touchscreen events to an existing TouchInteractionController for that display.
2. Computes `event.source & ~0xff`, removing input source-class bits.
3. Calls this service's `onMotionEvent` only if the result overlaps app-local `mMotionEventSources` and the event was not routed to the touch controller.

Thus a server-side observing subscription with local field still 0 would deliver no ordinary `onMotionEvent` callback through this gate. An exact owned-instance mask update is necessary. No touchscreen/controller path is enabled or changed by this finding.

`dispatchServiceConnected()` gets current service info, calls updateInputMethod, copies `getMotionEventSources()` into local `mMotionEventSources`, then calls `onServiceConnected`. This synchronization happens at connection dispatch; a later helper-side server update does not execute this method automatically.

## Setter bookkeeping and no-IME qualification

Existing public setter extract performs:

- Assign incoming info into local `mInfo`.
- Update input-method local state.
- Copy incoming motion mask into local `mMotionEventSources`.
- Send info through the app's own connection; on successful transport clear local `mInfo` and clear accessibility cache.

The helper route must not call that app-origin observing setter as a substitute: the already established server calling-permission gate sanitizes its observed mask.

Exact targeted `updateInputMethod` body checks `info.flags & 0x8000` (IME-editor flag). If requested and not initialized, it creates the IME; if not requested and previously initialized, it clears the IME and initialized bit. Otherwise it does nothing. Current own XML `apps/viewer-expo/android/app/src/main/res/xml/km_accessibility_service.xml` has no IME-editor or other accessibilityFlags declaration. Its connection-dispatch path with that flag absent leaves IME uninitialized.

Consequently **when the actual own service info continues to have no IME-editor flag and its local IME state is uninitialized**, updating own `mMotionEventSources` is the sole additional local motion-callback requirement exposed by these inspected methods. XML alone should not be substituted for checking the runtime flag assumption. This is not an assertion of complete public-setter equivalence: pending `mInfo`, cache invalidation, connection generations, rollback, and restoration remain implementation/lifecycle concerns. No broad private-state copy or dispatchServiceConnected re-invocation is justified by this result.

Field signatures proved by the targeted instructions:

```text
AccessibilityService.mMotionEventSources:I
AccessibilityService.mInfo:Landroid/accessibilityservice/AccessibilityServiceInfo;
AccessibilityService.mConnectionId:I
AccessibilityService.mInputMethodInitialized:Z
AccessibilityService.mInputMethod:Landroid/accessibilityservice/InputMethod;
```

Only type/member references are established for fields here; no whole-class declaration dump was made and no field annotation/accessibility success is asserted.

## Exact own connection-ID signature

```text
.method public blacklist getConnectionId()I
```

Its body reads own `mConnectionId:I` and returns it. Public bytecode modifier does not make this a public SDK-supported member; the blacklist marking is material. Earlier `AccessibilityInteractionClient.getConnection(int)` extract is public static `greylist-max-o`, returning a connection from a process-local static cache. App-side acquisition under actual target-SDK hidden-API enforcement remains a separate runtime proof. Reflecting a cache method in a shell process does not retrieve another app process's own connection.

No connection/member was acquired or invoked here. User approval for the exact proposal does not itself demonstrate successful hidden-member access, server application, callback delivery, or non-consuming physical behavior.

## Method evidence hashes

Directory `/private/tmp/leftcar-km-oem-observer-framework`; full bounded manifest `artifact-manifest.json`.

| Method-only artifact | Bytes | SHA-256 |
| --- | --- | --- |
| `service-motion-callback.smali` | 2372 | `56d06a0891d327cdbd24dfe0000fe938dd2aea5044e981e1de1784da9830909b` |
| `service-dispatch-connected.smali` | 2938 | `5210ccb354dafe1a98e04b02af855a83088254a6ef76ec74d2c2e769df966edd` |
| `service-getconnectionid.smali` | 191 | `2224a37f7457f23624fbf4ec0d681e663d2bc938b83ba2ed27e24d03d61b6408` |
| `service-update-ime.smali` | 1849 | `34e374c2df479779b8fd1c3775c775895cd820a56f8de5a5d936c0086f6c76dc` |
| `service-set.smali` | 925 | `0f212919c4a4aa1f6d5b712783c03900d7ebe0e15f8348f50fd6ab6d28b9f482` |
| `service-send.smali` | 2531 | `b6a1104c42b860364b39135b94225ce23ff48de49c71f6be2c3db41beacf76dd` |
| `client-getconnection.smali` | 896 | `29c9184059643aae8c64daca0d11415ac6b294a1998ba34ca02daf64911d27de` |

## Handoff and stopping point

Implementation prerequisite: couple exact own connection delegation to verified own-instance local dispatch bookkeeping, keep the no-IME assumption explicit, and keep restore/OFF synchronized. Existing capability, lifecycle, consent and input-source boundaries still apply. Do not generalize this evidence to other services or source masks.

Remaining evidence: compilation/package; actual own-app connection and field access; shell application/readback; current own-local mask readback; permission-preserving restoration; physical mouse passthrough/callback behavior. Static DEX bodies establish none of those runtime acceptance levels. Stop the read-only prerequisite here; Root owns the architecture and implementation decisions.
