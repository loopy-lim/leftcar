# Android preparation read-only probe — 2026-10-03

## Result

The user-reported combination `androidPreparation` + `helperUnavailable` proves failure within Android preparation and narrows the current presentation to seven exact native reasons. It does **not** identify which reason occurred. Existing `UserServiceManager` process-dispatch evidence does not establish constructor completion, binder attachment, or helper preparation. No safe current exception-class evidence establishes the cause; no fix or capability bypass is justified.

Scope: source/artifact inspection, official pinned-source inspection, and focused ADB read-only metadata/log reads. No source/runtime edits, installation, launch, UI operation, grants, input injection, commit, or tests. This file is the sole deliverable write. The diagnostic skill's reproduction/fix phases are deliberately not executed: the parent supplied the user reproduction, and this task expressly forbids retriggering preparation or adding instrumentation.

## Evidence and provenance

- Parent-supplied user observation: Start returns OFF with failure stage `androidPreparation` and closed issue `helperUnavailable`. The user-facing localized helper sentence is a shared category, not a distinct constructor error.
- Existing installation receipt `docs/research/2026-10-03-input-sharing-review/start-diagnostic-install.json` records update-only APK installation, byte/hash equality, Shizuku permission, and both enabled accessibility services. This probe did not repeat installation or change those services.
- Exact APK SHA-256: `3d60145f393eb386982741e03e8a597af308502b3bd414e78d714c14dcac9b91`; inspected archive: `/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/leftcar-android-internal-9afyDh/artifacts/viewer-apk-app-release.apk`. Receipt establishes this archive as installed; source-record checks below establish correspondence for the relevant files.
- Live `adb -s HA2H8QSD shell dumpsys package moe.shizuku.privileged.api`, reduced to version fields: versionCode `1086`, versionName `13.6.0.r1086.2650830c`.
- Live `adb -s HA2H8QSD shell ps -A -o UID,PID,NAME`, filtered to Shizuku/KM: shell UID `2000`, PID `25428`, `shizuku_server`; no persistent `km-authority` process at this read. Absence after cleanup does not establish that no child ever existed.
- APK manifest source hashes match this worktree's `KmRuntime.kt`, `KmInputAuthority.kt`, `proguard-rules.pro`, `app/build.gradle`, and Android `gradle.properties`.
- Current app dependencies explicitly use `dev.rikka.shizuku:api:13.1.5` and `provider:13.1.5`; the exact AARs exist in local Gradle cache. Client dependency version and installed server version are different evidence, not a mismatch diagnosis.
- No CONTEXT.md was found in the worktree. No prior memory facts are used in this report.

## Exact failure boundary

`apps/viewer-expo/src/km-failure.ts:25-31` maps these seven native reasons to the same `helperUnavailable` issue:

| Native branch | Source boundary | What would distinguish it |
| --- | --- | --- |
| 4-second helper connection timeout | `KmRuntime.kt:136`; generation still current and `authority == null` | bind-dispatch checkpoint, no service-connected checkpoint, timeout checkpoint |
| Helper binder death | `KmRuntime.kt:150-156` | service-connected then binder-dead checkpoint |
| Service disconnected | `KmRuntime.kt:173` | disconnected checkpoint |
| Binding died | `KmRuntime.kt:174` | binding-died checkpoint |
| Null binding | `KmRuntime.kt:175` | null-binding checkpoint |
| Synchronous bind invocation failure | `KmRuntime.kt:178` | bind-call-failed checkpoint + exception class |
| Foreground-service startup failure | `KmRuntime.kt:166-167` | helper preparation completed then FGS-call-failed checkpoint + exception class |

Prepare performs: active accessibility instance → Shizuku ping/shell UID/API >=13 → permission → KeyBridge lease decision → UserService bind → Binder death link → `api.prepare` → observer coexistence verification/configuration → FGS request → success callback. The 4-second timeout starts from initial prepare, so it also spans lease acquisition; it is not a measured constructor-only deadline.

KeyBridge incompatibility maps to `keyBridgeUnsupported`; helper `prepare()` returning false maps to `unsupported`; the worker's generic Exception reason maps to `startFailed`. These are not the supplied closed issue. Do not prioritize UHID/hidden-API failure or another permissions grant without evidence that service-connected was reached. Enabled accessibility settings alone do not prove an active `KmAccessibilityService.instance`, but its missing-instance branch also maps to a different issue.

## Official installed-version startup path

Official release [v13.6.0](https://github.com/RikkaApps/Shizuku/releases/tag/v13.6.0) resolves to commit `2650830c5b099ae0dd34fedf614d4f592ca05d65`, matching the installed version suffix. Its Shizuku-API submodule is pinned to `510fc988c02c3475d8c25db170f96792f105bdf8`. Investigation uses those immutable revisions, not an unverified master checkout.

The pinned [UserServiceManager](https://github.com/RikkaApps/Shizuku-API/blob/510fc988c02c3475d8c25db170f96792f105bdf8/server-shared/src/main/java/rikka/shizuku/server/UserServiceManager.java#L213) logs `Starting process` before writing the generated command to a parent `sh` process and waiting for that parent. The [ServiceStarter command](https://github.com/RikkaApps/Shizuku/blob/2650830c5b099ae0dd34fedf614d4f592ca05d65/starter/src/main/java/moe/shizuku/starter/ServiceStarter.java#L46) backgrounds `app_process`. Thus even parent-shell exit 0 cannot prove child constructor/binder success. Manager's separate `Received binder` boundary occurs later; it contains a token and must be classified in memory rather than copied raw.

The exact child log tag is **`ShizukuServiceStarter`**, set by [ServiceStarter.main](https://github.com/RikkaApps/Shizuku/blob/2650830c5b099ae0dd34fedf614d4f592ca05d65/starter/src/main/java/moe/shizuku/starter/ServiceStarter.java#L70). Its [pinned UserService.create](https://github.com/RikkaApps/Shizuku-API/blob/510fc988c02c3475d8c25db170f96792f105bdf8/server-shared/src/main/java/rikka/shizuku/server/UserService.java#L60) creates package context, initializes an Application through LoadedApk reflection, loads the target class, prefers its public Context constructor, and catches startup Throwable with an inability-to-start log. The starter exits 1 if this fails. A separate sendBinder path can fail after successful construction. Neither failure can be inferred from dispatch alone.

The packed APK DEX, inspected using `apkanalyzer dex packages --defined-only` and `dex code`, explicitly contains:

```text
.class public final Ldev/leftcar/viewer/km/KmInputAuthority;
.super Ldev/leftcar/viewer/km/IKmInputAuthority$Stub;
.method public constructor <init>(Landroid/content/Context;)V
```

Release minification is enabled. ProGuard keeps `KmInputAuthority { *; }` and the AIDL Stub; actual DEX confirms the public Context constructor survived. Shizuku v13 supports this constructor, so absence of a no-argument constructor is not a demonstrated defect. The constructor resolves the app UID from Context, requires an app UID >=10000 and process UID 2000, and creates the watchdog. Whether those checks passed on the failed launch remains unproved. Do not remove those checks or add a constructor workaround based on speculation.

## Existing-buffer probe

Two existing-buffer probes were run with exact tag filters and all other tags silenced. No raw logs were displayed or saved. Output was reduced in memory to static event IDs and Java/Android/Kotlin exception classes; any manager record tokens, shell command arguments, raw messages, and stack text were excluded.

1. `adb -s HA2H8QSD logcat -d -t 5000 -v brief -s ShizukuServiceStarter:I '*:S'`: no target starter events/classes found in that bounded retained slice.
2. `adb -s HA2H8QSD logcat -d -v brief -s ShizukuServiceStarter:I UserServiceManager:V '*:S'`: inspected maximum last 800 matching-tag lines; zero matching-tag lines available. `logcat -g` showed small rotating buffers with substantial unrelated use.

Empty current buffers do not disprove the parent's earlier manager observations or establish a child startup cause. No buffers were cleared. No further retrospective broad log dump is warranted.

## Next safe probe, before instrumentation

Use a **20-second live read-only tag capture** spanning exactly one user-operated Start attempt: `ShizukuServiceStarter:I` and `UserServiceManager:V`, all other tags silenced. Stop automatically; process only a bounded number of matching lines. Reduce before output to fixed event names (`record_created`, `process_dispatch`, `starter_entered`, `starter_create_failed`, `binder_sent`, `binder_send_failed`, `manager_binder_received`) and exception CLASS only, never exception messages/stacks, record tokens, APK paths, bind arguments, auth data, key events, or motion data. Do not save raw stream. Limit output to <=20 records. Start the capture before the user presses Start; absence in a post-attempt buffer is inadequate.

Interpretation: `starter_create_failed` + exception class narrows initialization/class/constructor failure; constructor completion plus `binder_send_failed` narrows attach transport; manager binder-received requires distinguishing the targeted fresh single attempt without exposing its token. Class alone can remain ambiguous (e.g. InvocationTargetException wraps several causes); include at most a bounded exception cause-class chain, still no message/stack. If starter never reaches its first fixed log boundary, the parent process launch/environment requires investigation; do not execute the Shizuku command manually because that would instantiate the authority.

This live collection was **not** run by this subagent; no user Start was triggered. Parent may run it within existing task authorization.

## Proposed next task only: static preparation checkpoints

If the live official logs remain insufficient, a reversible native diagnostic patch can add one fixed checkpoint per low-rate preparation boundary and exception CLASS only. Proposed unique tag `LeftcarKmPrepareProbe`, hard cap <=20 records per prepare attempt, active only while preparation is pending, no output from heartbeat/flush/watchdog/inject or input callbacks. Finite event names:

- `prepare_enter`, `preconditions_passed`, `lease_accepted`, `bind_dispatch`, `bind_call_returned`, `bind_call_failed`;
- `service_connected`, `death_linked`, `helper_prepare_returned`, `observer_check_passed`, `observer_configured`, `fgs_requested`, `fgs_failed`, `prepare_ready`;
- `bind_timeout`, `binder_dead`, `service_disconnected`, `binding_died`, `null_binding`, `worker_exception`.

At suppressed catches, emit checkpoint plus `error.javaClass.name` only. If typed exceptions are needed to distinguish helper false, use separate bounded checkpoints in helper `prepare` around observation and each UHID creation; only after service-connected evidence warrants that deeper probe. Do not weaken authorization, lease, coexistence, UID, observation, UHID, or cleanup gates; retain all current reason semantics. Do not log `Throwable.toString`, message, Binder identity, package/profile data, keyboard inventory, event payloads, or token/generation values.

The native source currently suppresses exception identity in worker `task`, synchronous `bindUserService` failure, FGS failure, and helper prepare catch. Therefore class-specific diagnosis is impossible from those paths without external official starter logs or such bounded checkpoints. For this actual closed issue, the additional immediate loss is the UI category merging seven precise fixed reasons. A fixed internal branch ID is sufficient to split that category safely; raw reasons are unnecessary.

No diagnostic patch is implemented here. Any patch, APK rebuild/update, and user retry are a separate parent-owned next execution step; acceptance still requires fresh runtime evidence from that exact artifact.


## Follow-up: actual one-Start capture and retained classification — 2026-10-04 KST

This section supersedes the earlier absence-of-retained-logs limitation for the subsequently observed attempt. Parent ran a 45-second read-only live two-tag capture spanning exactly one user-operated Start, producing `/private/tmp/leftcar-km-live-helper-probe.json`. That initial sanitizer emitted manager request/dispatch plus two unclassified starter records, without raw text. This subagent classified the still-retained exact two-tag lines using pinned official static strings, processing at most 800 matching lines and emitting at most 20 closed records.

`/private/tmp/leftcar-km-retained-helper-classification.json` contains:

| Relative to latest Leftcar record creation | Closed event |
| --- | --- |
| 0.000 s | `leftcar_record_created` |
| 0.000 s | `leftcar_process_dispatched` |
| 0.120 s | `leftcar_starter_entered` |
| 0.140 s | `leftcar_provider_null` |

A separate equality-only validation confirmed the null lookup authority is **exactly** the pinned `moe.shizuku.privileged.api.shizuku`, user 0, in both retained target attempts. The package-only authority wording in an earlier inter-agent message omitted `.shizuku`; this equality check corrects that wording. No raw line, token, arguments, exception message, or input data was emitted or saved. No constructor/startup exception class or manager binder-received event was found in the latest classified attempt.

**New proven boundary:** the target starter entered `UserService.create`, that call returned a service Binder, and the starter proceeded to `sendBinder`. The Context/class/constructor path therefore succeeded for this actual attempt. `sendBinder` called `ActivityManagerApis.getContentProviderExternal` for the Shizuku manager authority and received null, then returned false; the pinned starter exits 1 at that boundary. This proves failure to obtain the manager ContentProvider before Binder relay, not successful app-service binding. The observed app timeout category is consistent, but its exact native fixed reason is still not logged. Do not implement an app constructor/ProGuard/UHID workaround for this observed failure.

### Cheap before-state metadata

Read-only exact-package metadata, package-filtered provider activity metadata, installed APK manifests, selected process names, and user-state fields were reduced to safe fixed fields:

| Field | Observed value |
| --- | --- |
| Current Android user | 0 |
| User 0 state | `RUNNING_UNLOCKED` |
| Manager installed for user 0 | true |
| Manager hidden / suspended / stopped / notLaunched | false / false / false / false |
| Manager user 0 application enabled override | 0 (default) |
| Manager provider class | `moe.shizuku.manager.ShizukuManagerProvider` |
| Manager authority | `moe.shizuku.privileged.api.shizuku` |
| Manifest provider enabled / exported / directBootAware | true / true / true |
| Manifest provider process | default application process |
| Explicit provider component disable in user 0 dump | absent |
| Provider permission | `android.permission.INTERACT_ACROSS_USERS_FULL` |
| Shell package permission metadata | granted=true |
| Manager process in current selected-process snapshot | absent |
| Manager authority in current package-filtered active-provider dump | absent |
| Shell server | UID 2000, PID 25428, alive |

Supporting safe artifacts: `/private/tmp/leftcar-km-shizuku-provider-metadata.json`, `/private/tmp/leftcar-km-provider-manifest-fields.json`, `/private/tmp/leftcar-km-provider-user0-state.json`, `/private/tmp/leftcar-km-provider-authority-permission.json`. Installed manager/KeyBridge APKs were copied read-only to temporary files for manifest inspection; no package state or app data was written. Two unsupported convenience read commands (`cmd user is-user-unlocked` and `pm check-permission`) were replaced by actual `dumpsys` state/permission fields; their unsupported outputs are not evidence of denied permission or locked user.

For directly relevant comparison only, installed KeyBridge declares a separate exported provider `dev.loopy.keybridge.AdbOnlyProvider`, authority `dev.loopy.keybridge.shizuku`, with the same cross-user permission. It does not register or substitute the manager authority needed by this relay. KeyBridge was neither changed nor restarted.

These reads do not support missing user-0 installation, an explicitly disabled manager provider, a force-stopped flag, or a locked user as the immediate explanation. They do not prove why ActivityManager returned a null provider. Absence from current running-process/provider lists is a snapshot, not proof of the cause at lookup time.

### Exact compatibility boundary

Pinned [Shizuku settings](https://github.com/RikkaApps/Shizuku/blob/2650830c5b099ae0dd34fedf614d4f592ca05d65/settings.gradle#L29) selects `dev.rikka.hidden:compat:4.4.0`. Its [published source artifact](https://repo.maven.apache.org/maven2/dev/rikka/hidden/compat/4.4.0/compat-4.4.0-sources.jar), `ActivityManagerApis.java:60-75`, uses the four-argument `getContentProviderExternal(name, userId, token, tag)` on SDK >=29. It returns null for either a null ContentProviderHolder or a holder whose provider is null. It does not catch and convert exceptions into null. Therefore the observed branch is a returned null result, not an exception suppressed inside that helper. Android 16's [official IActivityManager declaration](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/app/IActivityManager.aidl) retains that four-argument method. No incompatible signature is established. OEM ActivityManager policy/provider startup behavior remains unproved; the generic null result cannot distinguish its underlying platform branches.

### Parent-owned next step

No further human retry is requested by this subagent. A parent-authorized, one-variable user opening of the Shizuku manager could compare provider/process before-state with after-state without restarting the shell server or KeyBridge; it would be an execution change and is not performed here. Any later Start retry should repeat the same bounded static classification and verify manager Binder receipt plus app preparation readiness. A server restart, provider invocation, force-stop, permission change, constructor bypass, or capability downgrade is not justified or executed by this report. The app-side checkpoint proposal above remains a separate possible task if exact native branch identity is still needed.


## Follow-up: provider recovery attempt — 2026-10-04 KST

Parent requested classification after the user-operated Shizuku-manager opening and Leftcar Refresh/Start recovery. This subagent did not operate UI or retrigger Start. The existing two-tag live observer reported new target dispatch, starter entry, and a Binder-sent candidate; those candidates were checked against the latest retained target attempt. Failure matching (`failed send binder`) deliberately precedes success matching to prevent substring confusion. Exact fresh-record token correlation stays entirely in memory; no token is output or saved.

Safe retained artifact: `/private/tmp/leftcar-km-provider-recovery-retained.json`. At most 800 matching-tag lines were processed, with at most 20 closed records output. Latest attempt:

| Relative to latest target record creation | Closed event |
| --- | --- |
| 0.000 s | `leftcar_record_created` |
| 0.000 s | `leftcar_process_dispatched` |
| 0.376 s | `leftcar_starter_entered` |
| 0.538 s | `leftcar_manager_binder_received` |
| 0.539 s | `leftcar_binder_provider_reply_received` |

No target failure, exception-class, or unclassified starter records appeared in this latest attempt. The manager-received event is correlated to this exact target record. **Binder relay to the Shizuku server is now proved for this attempt.** The send-Binder informational line itself precedes reply-Binder validation in pinned source, so the separately correlated manager receive boundary matters. Neither boundary establishes app callback execution, helper `prepare()` completion, foreground-service readiness, or successful input sharing.

Safe after-state artifact: `/private/tmp/leftcar-km-provider-recovery-after-state.json`.

| After-state field | Observed value |
| --- | --- |
| Shizuku manager authority registered in active-provider dump | true |
| Manager process | UID 10326, PID 14455 |
| Shizuku server process | UID 2000, PID 14660 |
| Shizuku server PID unchanged from prior 25428 | **false** |
| Persistent `leftcar.ll3.kr:km-authority` process at snapshot | absent |
| Leftcar KM accessibility enabled | true |
| KeyBridge accessibility enabled | true |

The server PID changed, so this observation must not be attributed exclusively to opening the manager or described as a controlled recovery with an unchanged server. The actor/mechanism behind the server replacement is unproved by these read-only checks. Both manager-provider availability and shell-server process differ from before-state. There was no restart by this subagent.

No further human retry is requested here. Await the current user-displayed closed failure/readiness. If it still fails, the failure is downstream of confirmed target Binder relay for this attempt; inspect the closed stage/reason before selecting another probe. Helper absence after the attempt remains compatible with cleanup and does not undo the relay proof. Native preparation/real input acceptance remains open.
