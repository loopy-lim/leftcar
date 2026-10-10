# OEM observer readback report — 2026-10-04 KST

## Result

The device's actual server-side implementation differs materially from the Android 16 r1 AOSP reference used in the prior probe. `AbstractAccessibilityServiceConnection.setServiceInfo` checks the **calling** process's `ACCESSIBILITY_MOTION_EVENT_OBSERVING` permission **before** clearing Binder identity. If a nonzero observed mask is supplied and that check is denied, it clears the incoming observed mask to zero, then copies the sanitized object into stored service info. The connection getter returns that stored object.

Combined with the already measured Leftcar permission=false, shell permission=true, successful shell fresh-object/Parcel test, and the newest exact `observer_configure / observation_readback_rejected` checkpoint, this provides a concrete source/runtime explanation: shell can construct the object, but the app's real service-settings Binder call is sanitized by the deployed framework. This conclusion uses actual gate code plus prior runtime facts, not permission absence alone. Exact observed getter values on that latest service call were not logged; the application of this code path to the specific attempt is a strong source/runtime inference, not an independently recorded getter trace.

No bypass, permission grant, service rewrite, alternate monitor, helper launch, or additional Start retry was performed or proposed here.

## Exact artifacts

Read-only pulls from device HA2H8QSD:

| Device file | Host artifact | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `/system/framework/framework.jar` | `/private/tmp/leftcar-km-oem-observer-framework/framework.jar` | 48528604 | `1b384aa5dd38e70b35ff00504ee4dcf1a51593545a09629cbd33fd523b496065` |
| `/system/framework/services.jar` | `/private/tmp/leftcar-km-oem-observer-framework/services.jar` | 25540201 | `d419950e2d77cb1dbfe56fdb053dabe832cc4291a767d850a00272378eb68bcc` |

On-device `sha256sum` exactly matched host hashes. Framework jar contains six DEX entries; services jar contains three. SDK `apkanalyzer dex code --class <named class> --method <exact signature>` successfully extracted only the named methods. No full jar/class dump or external tool installation/download occurred. Method-only files and hashes are preserved in `/private/tmp/leftcar-km-oem-observer-framework/artifact-manifest.json`.

The latest diagnostic APK is parent-identified by prefix `5fda0082`; this report does not imply that abbreviated digest is a fresh full-artifact validation. Existing tests, object probes and permission queries were not rerun.

## Targeted method findings

| Actual method | Method artifact | Relevant comparison with prior Android16 r1 reference |
| --- | --- | --- |
| AccessibilityService.getServiceInfo | `service-get.smali` | Fetches connection's remote info; returns null without connection; catches RemoteException. Relevant behavior matches. |
| AccessibilityService.setServiceInfo | `service-set.smali` | Stores info, updates input-method/local motion sources, sends it. No observed-mask clearing here. Relevant behavior matches. |
| AccessibilityService.sendServiceInfo | `service-send.smali` | Calls connection setter with current info, clears local cache afterward, catches RemoteException. No local observed-mask clearing. Relevant behavior matches. |
| AccessibilityServiceInfo.updateDynamicallyConfigurableProperties | `info-dynamic.smali` | Copies motion mask directly, then calls observed setter when observing flag=true. Relevant behavior matches. |
| AbstractAccessibilityServiceConnection.setServiceInfo | `connection-set.smali` | **New compared with r1:** checks calling permission and zeros incoming observed mask before clearCallingIdentity. |
| AbstractAccessibilityServiceConnection.setDynamicallyConfigurableProperties | `connection-dynamic.smali` | Copies incoming motion/observed getters to internal connection fields without the prior r1 nested permission check. |
| AbstractAccessibilityServiceConnection.getServiceInfo | `connection-get.smali` | Returns stored mAccessibilityServiceInfo under the lock. |

This comparison is semantic and bounded to relevant behavior; it is not byte-equivalence to any official Android source release, nor a claim that every change originates with this OEM rather than a later platform patch level. The prior immutable [Android16 r1 AbstractAccessibilityServiceConnection](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/services/accessibility/java/com/android/server/accessibility/AbstractAccessibilityServiceConnection.java) is the reference already inspected in the previous report; no network fetch was needed for this task.

## Actual pre-identity gate, in order

The extracted method's debug line markers are 523-530 (artifact line numbers can be obtained from the preserved smali file):

1. Read `incomingInfo.getObservedMotionEventSources()`.
2. If zero, skip the permission block.
3. Otherwise call `mContext.checkCallingPermission(ACCESSIBILITY_MOTION_EVENT_OBSERVING)`.
4. If granted (0), skip the clearing block.
5. If denied, call `incomingInfo.setObservedMotionEventSources(0)`.
6. Only then call `Binder.clearCallingIdentity()`.
7. Update stored service info's dynamic fields from the sanitized incoming info, then copy it into the internal connection configuration.
8. Restore identity.

The exact original-caller permission check replaces the earlier r1 ambiguity about `checkCallingOrSelfPermission` under cleared identity. The prior report's identity caveat was correct for that reference but cannot be used to dismiss the actual device's earlier calling-permission check.

Because the actual getter returns the stored info after incoming sanitization, this implementation can expose observed=0 through the app's readback, consistent with the exact local `observation_readback_rejected` checkpoint. It is not necessary to assume null service info, parcel field loss, constructor failure, or unsupported shell reflection to explain that rejection.

The ordinary motion mask is not cleared by this gate; observed and ordinary subscription have different semantics. Therefore a requested observing mode must not be treated as safely supported merely because the helper can set object fields or the ordinary motion mask is retained. The existing verification rejection is material capability evidence, not a cosmetic false negative to suppress.

## Evidence limits and stopping point

- The static files are exact deployed system artifacts verified by device/host hash, but no runtime breakpoint was attached to system_server and no per-call observed value was collected here.
- Prior OEM permission metadata remains signature protection, Leftcar granted=false, shell granted=true; no query or grant repeated.
- Existing same-device fresh data object evidence remains valid for that limited object/Parcel path; it does not authorize the app's Binder call.
- The latest native classification rules out the two explicit service-info-missing branches for that attempt; it establishes the verification rejection branch. It does not itself log which Boolean comparison rejected.
- No monitor path is established or instantiated. No permission or capability gate is bypassed.

Enough actual method-body evidence is available. Stop this read-only probe; preserve artifacts for parent review. Any product change should keep the unsupported observation boundary explicit and must not convert this gate into consuming or broader input capture without a separately reviewed contract. No implementation recommendation is presented as completed work.


## Quick scoped shell-application feasibility — facts only

No existing public/platform-approved route for the helper to obtain and apply only this app's active accessibility connection was readily established. The public AccessibilityService setter always dispatches from its owning app process; preparing its data object in shell does not change the later Binder caller.

Additional two method-only reads from the same framework artifact:

- `service-onbind.smali`: AccessibilityService.onBind is public **final** and creates the framework IAccessibilityServiceClientWrapper. It cannot be replaced with an app override merely to intercept that connection handshake.
- `client-getconnection.smali`: AccessibilityInteractionClient.getConnection is `greylist-max-o` and reads the **process-local static connection cache**. Reflecting that method in a separate shell process does not locate the app's cached active connection. It is not a public cross-process discovery API.

At the raw Binder endpoint, the actual calling-permission check would see shell if shell possessed and invoked the exact already-authorized service connection. An explicit delegation of that connection is therefore a theoretical architecture seam, not an existing established public API route or completed safe design. Obtaining the private connection capability, limiting it to this own service, preserving its lifetime, and proving supported framework access are unestablished; no capability was acquired or called. This report does not endorse defeating hidden-API restrictions or transferring arbitrary service connections.

No signature permission grant, global input monitor, actual service setting, or broader input API was invoked. Root owns architecture decisions; this feasibility pass stops at those bounded facts.
