# Task5 fix1 frozen report

Status: F1/P1 and F2/P2 fixed in the same approved own-service delegation architecture; frozen for exact-delta re-review. Root's requested official-support sentence is added to setup documentation. No release build, install, device/UI/runtime/input action, permission/policy/exemption write, React/Host/helper capability change, subagent, commit or publication performed. This remains a Shizuku shell + restricted non-SDK route, not a public-SDK-supported integration or physically accepted behavior.

## Exact delta and preimages

All modified files saved before edits under `task-5-fix1-before/`. Every fix1 preimage matches the preceding Task5 frozen SHA in the previous hash manifest; original Task5 baseline remains preserved separately in `task-5-before/`. New fix1 patch/hash manifest: `task-5-fix1-only.patch`, `task-5-fix1-hashes.json`. Fix1 patch SHA-256 `6a83e991b235e6588c640446f78f3469d7dc56972afc04531f9e1ce4d8cfa9da`. Consolidated `task-5-only.patch` SHA-256 `0330b241e361817c05516e6b57cbc348c0731a76c6fccb24bfe6a5e0be5ac4f3`; consolidated `task-5-hashes.json` updated for all original11 files.

| Fix1 file | Exact frozen preimage SHA-256 | New frozen SHA-256 |
| --- | --- | --- |
| apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt | `71dd96f5525284bf548ef5801b63bd8a579ed64f8b73ddcc70ba13dda910a68f` | `21c153b1a28620babd02a0e91c4c0d95183a1c6677c259cc4e5764e071401e58` |
| apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt | `b1eccaff070423b4a84b382ad031d8ac1f4aa17e2f60d7ed8d84e74d70908492` | `d664a9d0968ac40f22eef1e72c0d85fe4ae30b15a16dac760bfe2fd51228c145` |
| apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmOwnObservation.kt | `0f43da815f38bcb2b8e54bf7d8c2102e26c2c88874583bbf777a11c56b058e80` | `4642310aee3c575fd195f2520125650d6185e0b5d4f85c1a60d464262b6b7dad` |
| apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/KmOwnObservationTest.kt | `080b21bfdd14a9c220a480d1c097bd202abfa7a96b5e5deb57babd55531a7233` | `51444275be09a1381d72a3e5e6fdd3c8ac7ce9f7de1080100addc2e09b625401` |
| apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/KmRuntimeRecoveryTest.kt | `66a8578a8aa442b09ab2822d0b320588c2d02d71092cbd4f228c7081c77cb95a` | `45e45f9ff7ca2b652d92d27fb293fce64699021fbff0647d812510ff27115f93` |
| apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/KmRuntimeCompletionTest.kt | `75de448ba9a49a86472088828fb1d97c6dae0dd6b261aa664ad0b67afa9d109c` | `d71927b06f244a7b918e09072b9b63c91255e1f870ce01058ada05e3b4a2df7a` |
| docs/input-sharing-setup.md | `69e9397156967b610ea3cc30304ed25b8cb2f1b6a4870cfadfc78e51b33281bb` | `fb59112cddab10a59a5b523630a6d0b3ba1331d5302dcd893c4e87b074f5bea0` |

## F1 — synchronous session admission and short cancellation commitment

`KmObservationSession` holds a volatile cancellation revision. Explicit Runtime.prepare captures the admission ticket before it queues worker work; that ticket stays immutable through the prepared session. Both Runtime.stop (synchronously, before posting cleanup) and stopNow advance cancellation. configure.begin only advances a per-attempt token and cannot reset this separate session revision. Queued prepare/bind continuation, own service's nonzero local/helper write fences and ownership path read the session ticket; a Stop completed while helper.prepare/conflict/capture work is in progress therefore prevents subsequent active admission for that same session. Only a later explicit prepare obtains a fresh ticket.

Final member/local/system reads are outside commitment gates. Final routing+owner changes run inside the shared session gate then the own attempt gate; they recheck local instance, cancellation revision and attempt token, and retain the existing atomic physical capture-ticket/hold check. All three ownership targets now set owner in the same protected routing callback, removing the previous separate worker-only assignment after configure. Runtime ready/status and authenticated State completion also have cancellation-guarded publication. ObservationSession.current is a nonblocking volatile read, so own main local writes holding the attempt gate never wait for the session gate. Stop first takes/releases session cancellation, then takes/releases attempt cancellation; final commit uses session→attempt→routing, with no reverse nested locking. These short final gates hold no reflection/member read, Binder/system RPC, main-thread completion wait, helper input/watchdog monitor or network send. The original bounded main/write token and post-failure public OFF paths remain.

`observationService` is now volatile because Stop callers may read it outside the worker. It remains only the exact cleanup service reference; session cancellation is the primary synchronous authority regardless of service publication timing. Generation and capture gates retain their prior jobs. Helper RPCs remain outside the input/watchdog monitor; in-flight RPC latency is not asserted bounded. An issued RPC that returns after cancellation fails the fresh session/attempt commitment and enters existing verified OFF recovery.

## F2 — independently required app observed getter preflight

The real service calls observationPlatform.observed on its freshly obtained current own ServiceInfo before any nonzero main local dispatch write or helper mutation, after current-instance/generation and no-IME guards. This uses the exact app-side targeted observed getter rather than trusting shell access. Its exception/class-init failure enters the existing fixed KmObservationUnavailable.REASON path, with no owner commit; OFF restoration still attempts and requires that same getter and will remain unconfirmed if it is unavailable. Missing observed access is not hidden by a successful ordinary zero write or a helper result.

The regression fixture lets connection/local field access succeed and makes only observed() throw; it records every active local write and helper active call and asserts zero of both, no active routing/owner, and the fixed failure. No arbitrary hidden fields or replacement discovery/readback route was added.

## Meaningful red/green verification

Working directory `apps/viewer-expo/android`. Approved normal Gradle cache escalation only; no init script or task exclusions.

Red, before production corrections:

```sh
./gradlew :app:testDebugUnitTest --tests 'dev.leftcar.viewer.km.KmOwnObservationTest.only app observed*' --tests 'dev.leftcar.viewer.km.KmOwnObservationTest.cancel after final*' --tests 'dev.leftcar.viewer.km.KmRuntimeRecoveryTest.Stop returned*' --console=plain
```

`task-5-fix1-evidence/red.log` and preserved red XML: **3 tests completed, 3 failed**, BUILD FAILED in10s, each at its intended behavioral assertion. (1) Only app observed getter missing still returned active success. (2) Actual final local read cancels the service after freshness, but old routing/owner commitment returned success. (3) A separate Stop thread returns while the actual Runtime worker remains inside helper.prepare; old configure admission then records nonzero local/helper writes. No compilation-only red or implementation-mirroring test is counted.

Final affected native suites, run once after all source/test corrections:

```sh
./gradlew :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmOwnObservationTest --tests dev.leftcar.viewer.km.KmRuntimeCompletionTest --tests dev.leftcar.viewer.km.KmRuntimeRecoveryTest --tests dev.leftcar.viewer.km.KmPhysicalTransitionTest --console=plain
```

`task-5-fix1-evidence/green.log` and current XML: **30 tests, zero failures/errors/skips** (OwnObservation14, RuntimeCompletion7, RuntimeRecovery6, PhysicalTransition3). BUILD SUCCESSFUL in10s, 206 actionable tasks, 7 executed/199 up-to-date. Native production and test Kotlin compilation passed. The final readback cancellation regression tests both OBSERVE and CONSUME; an additional real authenticated Runtime State/socket seam verifies Runtime.stop at that exact boundary rejects completion without owner success. Existing cleanup/death, local holds, stale generations, observed readback, helper endpoint guards and bounded-main tests remain covered by these affected suites. No unrelated/passed suites were repeated.

Existing Gradle deprecation/problems warnings remain reported and unsuppressed. React source untouched; no React Doctor claim or rerun belongs to this fix1 delta. Setup's requested official-support clarification is prose-only and needs no added test. These JVM/OS-seam tests do not establish ART access, packaged source identity, shell application or physical device behavior; Root's separately owned re-review, normal integrated build/signature/pulled-byte and human/device acceptance gates remain pending.
