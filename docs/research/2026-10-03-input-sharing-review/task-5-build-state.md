# Task5 package and update-only installation

Normal Root `bun run build android-internal` completed: BUILD SUCCESSFUL in2m23s,430 tasks (33 executed). Existing ARCore1.53.0 two R8 stack-map warnings and Gradle deprecations remain unsuppressed.

Source1123 file hashes matched Root, including every reviewed Task5 final file. JavaScript HBC and Rust native digests match the earlier verified artifact; temporary preparation tags/closed diagnostic labels are absent from DEX. Existing certificate remains `dd0f47dc0791450eac89ac2d304930d642ff32ad888a618a1992a6225f3b9655`.

APK `c16c5bc0ff608b92f6e176258615c1d34cf561004cd4f15a9d1c837ea65da3b1` was installed with `adb -s HA2H8QSD install -r` (Success) and pulled back byte-identical to the verified archive. KeyBridge and Leftcar accessibility services and Shizuku API grant remain enabled. No data reset, installer launch, automatic grant or Host update.

[Package verification](task-5-package-verification.json), [installation receipt](task-5-install.json), [normal build manifest](task-5-android-manifest.json).

Source review and30 scoped tests passed; actual app-side ART access, shell observation application, native edge/hover/click/wheel/key and recovery acceptance remain pending. Human asked to reopen/reconnect and Refresh→Start once on this exact artifact. This implementation depends on Shizuku shell and restricted non-SDK interfaces; it is not a public-SDK-supported Android integration.
