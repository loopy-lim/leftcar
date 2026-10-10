# Native-handshake cryptographic provider probe — 2026-10-04 KST

## Result

The production transformation `ChaCha20-Poly1305` is **available and functional on the actual Android 16 device** via AndroidOpenSSL. The alternate `ChaCha20/Poly1305/NoPadding` also works on that device. The unchanged production Kotlin `KmCipher` class successfully derived keys, sealed/opened a dummy frame, enforced replay rejection, and opened a dummy canonical-transform frame. **Unsupported production algorithm is not supported as the current nativeHandshake failure explanation.** No transform-name fix is justified by these results.

Host JBR differs: production transformation succeeds through SunJCE; the alternate canonical transformation fails at getInstance with `java.security.NoSuchAlgorithmException`. Replacing the production name based only on the Android documentation would introduce a host-provider mismatch.

The live app's generic failure remains unclassified. This probe uses the same device's default platform provider in shell, not the actual app socket/session or current production secret. It proves the cryptographic primitive and compiled KmCipher contract in that environment; it does not prove Host interoperability or successful live handshake. Root reports no deliberate provider replacement in app source, but this probe does not inspect app provider order at runtime.

## Scope and artifact provenance

Parent identifies exact installed Task5 APK by prefix `c16c5bc0` and reports Android preparation/Host approval passed, followed by generic nativeHandshake failure. No installation/archive checks were repeated here and the short digest is not presented as full artifact verification.

Actual production `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmCipher.kt` was copied unchanged to temporary scratch and compiled directly using cached Kotlin compiler 2.1.20. Snapshot SHA-256 `00d641e333a7b83d6f9b2696e6e9cc06222c0731a8a147b529732ffd1f55190b`. Java harness calls that class, not a substitute production implementation. The direct transform comparison and independent dummy HMAC contract calculation are additional harness checks.

Scratch `/private/tmp/leftcar-km-native-crypto-probe/` includes snapshot, Java harness, compiled classes, single dex, safe Android/JBR result JSON, and `artifact-manifest.json` containing exact file hashes/bytes. Compiler/JBR/SDK tools were already installed; no dependency was added to product source. Fixed dummy seed/plaintext/session inputs are used only in memory and never printed. No actual keys, session data, ciphertext, nonce values, or messages were output or saved.

Temporary `/data/local/tmp/leftcar-km-native-crypto-probe-20261004.dex` was pushed, run through shell app_process, removed, and tested absent. Push/run/removal exit 0; absence=true. No app launch, UI, settings, permission grant, input/monitor/device open, network socket, Shizuku mutation/restart, production source edit, or additional human Start occurred. Host probe ran the same dummy harness separately using Android Studio JBR.

## Exact provider/signature results

Harness uses the production signatures: `Cipher.getInstance(transform)`, `SecretKeySpec(key, "ChaCha20")`, 12-byte `IvParameterSpec`, ENCRYPT_MODE + doFinal, DECRYPT_MODE + offset/length doFinal. No AAD is added.

| Environment | Transformation | Provider | getInstance / init / seal / open |
| --- | --- | --- | --- |
| Android HA2H8QSD | ChaCha20-Poly1305 | AndroidOpenSSL | success; dummy roundtrip=true; 16-byte tag=true |
| Android HA2H8QSD | ChaCha20/Poly1305/NoPadding | AndroidOpenSSL | success; dummy roundtrip=true; 16-byte tag=true |
| Host Android Studio JBR | ChaCha20-Poly1305 | SunJCE | success; dummy roundtrip=true; 16-byte tag=true |
| Host Android Studio JBR | ChaCha20/Poly1305/NoPadding | unavailable | getInstance throws java.security.NoSuchAlgorithmException |

Actual compiled Kotlin class on Android:

| Contract check | Safe result |
| --- | --- |
| Both directional keys match independent dummy HMAC/HKDF calculation | true |
| Directional keys distinct | true |
| Actual seal/open dummy roundtrip | true |
| Frame overhead and first big-endian counter match | true |
| Second open of same frame rejected | true |
| Canonical-transform frame opens through actual production class | true |

Actual Kotlin production class on JBR also passes derivation, frame/roundtrip, and replay checks. Its canonical-cross-transform test fails before producing the alternate frame because SunJCE does not accept the alternate transformation. No Android exception was emitted in the tested primitives.

Safe outputs: `android-result.json`, `jbr-result.json`; maximum 16 closed records each. Provider names, transform names, fixed checkpoint strings, exception classes, and result booleans are the only runtime result fields.

## Contract comparison with Host source

`crates/secure-channel/src/lib.rs` `km_keys` uses HKDF-SHA256 with UTF-8 session ID as salt, session seed as IKM, info `leftcar/km/v1`, and 64-byte output split into two 32-byte direction keys. Kotlin's extract HMAC then first/second expand blocks implement that structure; the independent dummy calculation matched both actual outputs.

Host `StreamSealer` starts the counter at 1, uses an unsigned 64-bit big-endian counter prefix, constructs the nonce as four zero bytes followed by counter big-endian bytes, and authenticates without AAD. Kotlin uses the same nonce/prefix shape and appends ciphertext/tag. Android `KmDuplex` adds a four-byte big-endian frame length externally through DataOutputStream. Its production RX uses the second key and TX the first, matching c2s/s2c documentation.

This is a source-contract comparison plus actual Kotlin primitive test. It is not a newly executed Rust↔Android or current Host-session interop test. The probe's local actual roundtrip uses one same directional key for paired seal/open; it does not mix the two independent keys or claim traffic acceptance.

## Primary-source context

[Android Cipher documentation](https://developer.android.com/reference/javax/crypto/Cipher) lists ChaCha20 with Poly1305/NoPadding support from API 28. That canonical name alone does not establish that the hyphenated name is unsupported.

An immutable [official Conscrypt OpenSSLProvider source revision](https://android.googlesource.com/platform/external/conscrypt/+/daea556c33d7dc013964e2cb6f9a9ab70bbcd4bb/repackaged/common/src/main/java/com/android/org/conscrypt/OpenSSLProvider.java) registers the canonical transform and the hyphenated alias. This source illustrates why both may work; it is not asserted to be the exact installed Conscrypt source revision. The actual device lookup/init/roundtrip results above are the decisive current provider evidence.

## Remaining failure boundary

`KmRuntime.start` catches multiple configuration/socket/initial-send failures under a single generic reason. `KmDuplex` reader and writer loops separately suppress concrete exceptions into fixed failure reasons. State-readiness timeout and Host authorization lifecycle are separate from transform availability. No socket or app session was created by this probe, so those paths remain open.

Stop this bounded provider probe. Preserve its concrete negative finding against speculative algorithm renaming; Root owns any next closed-checkpoint classification and source fix.
