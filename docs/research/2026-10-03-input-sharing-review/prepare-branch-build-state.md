# Closed preparation branch diagnostic build

Normal Root `bun run build android-internal` succeeded in 2m15s, including release Kotlin compilation and normal release packaging, with no task exclusions or temporary init scripts. Exact reviewed source939f95 and1121 current file hashes match the archive manifest. APK5fda0082ed1e37d7e3bfa4d903a6ff21ba2818b70c9e3e06801ab4647126aa35 verifies with existing certificate dd0f47…9655. Hermes and Rustnative digests are unchanged from a6ab83 diagnostic APK, and the fixed tag plus five closed branch labels remain in DEX.

External ARCore1.53.0 emits two R8 stack-map warnings and Gradle reports deprecations; no pristine-build claim. No React or JS change; the last exact React100/typecheck/59-test gate applies to identical source and HBC. No redundant native or React suites were run for this temporary log field.

Update-only install returned Success. Pulled installed bytes equal archive; known Leftcar and KeyBridge services and Shizuku grant remain enabled. No reset, automatic grant, installer app launch, Host update, or runtime acceptance. Bounded120s/max20 fixed-record observer67295 was started before requesting one human Start. Exact failure branch remains pending; remove all temporary diagnostics after minimum cause evidence.
