# Observer configuration probe — 2026-10-04 KST

## Result and scope

The actual latest diagnostic Start reached connected authority, helper preparation, and observer coexistence verification, then failed at `observer_configure` with `java.lang.IllegalStateException`. Exact source branch is still unproved. The same OEM's fresh `AccessibilityServiceInfo` object correctly preserves both mouse motion and observed-source masks through flags modification and Parcel round trip. This excludes basic shell-side setter failure or Parcel loss for the tested fresh object; it does not validate a real service's applied configuration or actual non-consuming observation.

Scope: current source read, immutable Android 16 primary sources, exact-field package permission metadata, and two explicitly authorized temporary shell probes. No product source edit, app/helper launch, actual authority instance, real service settings write, UHID/device open, input injection, monitor creation/events, grants, restarts, UI, network/session operations, or user retry. Only temporary diagnostic dex files were copied to device; each was removed and absence verified.

## Actual failure evidence

Parent identifies the newly installed same-signer diagnostic APK by abbreviated digest `a6ab83…3e2a`. This subagent does not promote that abbreviation into a full artifact hash verification. `/private/tmp/leftcar-km-native-prepare-observer.json` is a safe fixed-checkpoint artifact:

- `service_connected` +27.363 s;
- `death_linked`, `helper_prepare_enter` +27.364 s;
- `helper_prepare_returned` +27.365 s;
- `observer_check_passed`, `observer_configure_enter` +27.365 s;
- `worker_exception`, stage `observer_configure`, exceptionClass `java.lang.IllegalStateException` +27.367 s.

The runtime proceeds beyond helper prepare only when it returns true. Failed UHID/helper preparation is therefore not the current demonstrated boundary. The single outer stage includes several operations and does not identify the throwing statement.

## Current source candidates

`KmAccessibilityService.configure` reads `serviceInfo`, passes it to authority `configureObservation`, changes the key-filter flag, calls `setServiceInfo`, reads `serviceInfo` again, verifies it through the authority, and changes routing mode.

| Candidate boundary | Present source behavior | Current evidence limit |
| --- | --- | --- |
| Initial serviceInfo missing | Kotlin `error` throws IllegalStateException | Active service instance does not guarantee live connection/readback; no inner checkpoint |
| Authority configure | authorization/capability `check` can throw; setter uses reflection | Prior helper prepare/coexistence passed; no current branch-specific proof |
| Actual setServiceInfo | framework Binder operation; AOSP has an IllegalStateException parcel-size guard | Fresh data object alone does not reproduce real service call |
| Applied serviceInfo missing | Kotlin `error` throws IllegalStateException | Getter result not recorded |
| Verification mismatch | Boolean authority verify false causes IllegalStateException | Actual getter comparisons not recorded |
| Routing | OBSERVE follows `setMode`, not CONSUME's capture-ticket check | No specific routing exception evidence |

Authority `configureObservation` sets motion mask first, then hidden observed mask. `verifyObservation` requires supported flag/API, motion == requested mask, and hidden observed getter == expected mask. Those are data-object checks, not direct verification of the system's input filter/connection state.

## Primary Android 16 behavior

Pinned reference: `android-16.0.0_r1`.

- [AccessibilityServiceInfo](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/accessibilityservice/AccessibilityServiceInfo.java): public motion setter clears observed; hidden observed setter enforces subset of motion; both fields are written/read in Parcel. Dynamic update copies motion directly and then applies observed setter when the observing flag is enabled (lines 879-895).
- [AccessibilityService](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/accessibilityservice/AccessibilityService.java): getter (2617-2628) fetches the server's service info through the current connection and returns null without a connection. Setter (2639-2643) caches local info, updates local motion sources, and sends it; send method (2651-2663) calls the service connection when present. It does not require another motion setter on the object.
- [AbstractAccessibilityServiceConnection](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/services/accessibility/java/com/android/server/accessibility/AbstractAccessibilityServiceConnection.java): setServiceInfo (526-557) checks parcel size, clears Binder calling identity, updates the stored info's dynamic fields, then updates connection/filter fields under the lock before restoring identity. The getter (506-512) returns stored info.

### Security-relevant internal gate

In that same connection source, `setDynamicallyConfigurableProperties` (412-435) sets internal generic sources from info, then—if the observing feature flag is enabled—checks `ACCESSIBILITY_MOTION_EVENT_OBSERVING` using `mContext.checkCallingOrSelfPermission`. If granted it copies the observed mask into internal `mObservedMotionEventSources`; otherwise it sets that internal value to zero. That denial branch does not clear the observed field in the stored `AccessibilityServiceInfo` object. **The published getter/readback can therefore retain an observed mask while an internal connection mask is zero if this gate denies.** Data-object equality alone is insufficient safety proof of non-consuming observation.

However, `setServiceInfo` clears the caller's Binder identity before invoking this dynamic update. The nested permission check must be interpreted in that actual identity context; the app's package grant cannot simply be substituted for it. The denial branch is not established as the current failure cause. An earlier inter-agent statement that a missing app manifest request might make this path unavailable was too strong without that identity boundary; this report corrects it. No permission bypass or grant is proposed.

## Same-OEM fresh object probe

Temporary source `/private/tmp/leftcar-km-observer-info-probe/KmObserverInfoProbe.java` compiled using Android Studio JBR, Android SDK 36 android.jar, and build-tools 36.0.0 d8. It ran via shell app_process UID 2000 with no Context or Binder/service acquisition. It only constructs a new data object, invokes getters/setters, modifies its flag field, and writes/reads an in-memory Parcel.

Safe result `/private/tmp/leftcar-km-observer-info-probe/result.json`:

| Data object step | Motion equals mouse-family mask | Observed equals mouse-family mask | Observed zero |
| --- | --- | --- | --- |
| Fresh object | false | false | true |
| Public motion setter | true | false | true |
| Hidden observed setter | true | true | false |
| Object key-filter flag modification | true | true | false |
| Parcel-created copy | true | true | false |
| Public motion setter called again | true | false | true |

Hidden setter/getter lookup succeeded; no exception was emitted. The source mask is exactly `SOURCE_MOUSE | SOURCE_MOUSE_RELATIVE | SOURCE_TOUCHPAD`, matching current code; no touchscreen mask was added. No real service settings were touched. Temporary device dex removal exit 0 and subsequent absence check true.

## Actual permission and candidate API metadata

Exact OEM permission blocks were read, bounded to the named permissions. A second temporary reflection probe performed read-only `IPackageManager.checkPermission(permission, package, user0)` calls and checked only two named InputManager signatures; it neither acquired nor instantiated any monitor. Safe result `/private/tmp/leftcar-km-observer-info-probe/permission-probe-result.json`:

| Field | Actual result |
| --- | --- |
| ACCESSIBILITY_MOTION_EVENT_OBSERVING OEM protection | signature |
| Leftcar user0 observation permission granted | false |
| Shell user0 observation permission granted | true |
| MONITOR_INPUT OEM protection | signature|recents |
| Shell user0 MONITOR_INPUT granted | false |
| monitorGestureInput(String,int) present | true |
| legacy monitorInput(String,int) present | false |
| Known two-argument gesture candidate source-filter parameter | absent |
| Monitor instantiated | false |

The shell's observation grant and setter access do not transfer to app UID service calls. Likewise, a system signature-protected permission is not a routine grant request to make the product work. Current app manifest does not request the observation permission, but its grant absence does not by itself resolve the cleared-identity dynamic gate described above.

[InputManager](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/hardware/input/InputManager.java#1129), [InputManagerGlobal](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/hardware/input/InputManagerGlobal.java#1737), and [IInputManager](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/hardware/input/IInputManager.aidl#167) expose the examined gesture monitor candidate without a source-filter argument. No shell-capable, mouse-only alternative is established by this narrow presence check. A display-wide monitor with filtering after receipt would not meet the requested source boundary; no such monitor was constructed or proposed as a workaround. This is not an exhaustive search of every OEM API.

Both temporary device dex files were removed, each with successful absence verification. Host temporary Java/class/result files remain for audit; no product source is changed.

## Next discrimination, without guessing a fix

Enough fresh-object/Parcel evidence has been obtained; stop this probe. A parent-owned next diagnostic patch, if needed, can add bounded fixed checkpoints after initial info read, authority configure return, setServiceInfo return, applied info read, and verification result. Boolean fields may record initial/applied info non-null, actual motion equality, observed equality, and supported flag only; exception CLASS remains sufficient. It must preserve existing stop/coexistence gates and avoid logging service info objects, package inventories, events, raw messages or stacks.

If all object checks pass, actual internal non-consuming observation still needs independent system boundary evidence; getter equality alone is not acceptance. If missing serviceInfo or a particular getter mismatch is established, select the smallest repair for that branch. No additional human Start, source patch, service setting, permission, or monitor action is performed here.
