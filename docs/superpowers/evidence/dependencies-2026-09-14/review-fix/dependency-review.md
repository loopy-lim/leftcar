# Independent dependency review — 2026-09-14

Reviewed the six-file scope supplied by the parent in `dependency-review.diff`, against `dependency-review-files.json`. This review covers the pinned input, not subsequent fixes. No source file was modified and no Gradle, build, device, display, app-launch or permissions action was run.

Result: **Critical 0 / Important 2 / Minor 0.** Both findings concern license metadata correctness. The targeted dependency upgrades themselves have no additional finding in this bounded review.

## Important 1 — reject duplicate singleton POM elements

Location: `tools/release-gradle-licenses.mjs:7-9`, used at lines 36-43.

`child(element, name)` returns the first matching element. A well-formed XML document with conflicting duplicate `groupId` elements therefore passes the exact-coordinate check when the first value matches. Likewise, two `licenses` sections silently select the first license. Both cases are reported as declared MIT by the current collector instead of remaining unknown. XML syntax validation does not validate the POM model's singleton constraints.

The independent fixture `dependency-review-fixtures.json` contains the exact XML and observed metadata. `duplicateIdentity` contains `org.example` followed by `org.wrong`; `duplicateLicenses` contains MIT followed by Apache-2.0. Both returned `cached-pom` / MIT. The valid control returned MIT, while malformed XML and a DOCTYPE returned no metadata.

Requested correction: reject ambiguous singleton identity, parent and licenses elements, including parent-coordinate and license-name fields where a single value is expected, before claiming declared metadata. Add focused regression cases for conflicting duplicates rather than only duplicate cache-file bytes.

## Important 2 — preserve a child's explicit license list when names are unavailable

Location: `tools/release-gradle-licenses.mjs:42-46`.

Parent lookup is triggered whenever the collected license names are empty. This differs from Maven inheritance: a parent supplies licenses only if the child's license list is empty. A child can declare a license with a URL and no name; it then has its own license list even though this collector cannot produce a name. [Maven's own merger](https://maven.apache.org/ref/3.9.11/xref/org/apache/maven/model/merge/MavenModelMerger.html) checks the list itself in `mergeModel_Licenses`.

The independent `dependency-review-inheritance.json` fixture has an exact child coordinate, a child license URL and an exact parent with MIT. On the pinned helper SHA the collector incorrectly returned MIT / `cached-parent-pom`. This can misattribute the child's license and make a metadata gap look resolved.

Requested correction: distinguish an absent/empty child license list from an existing list without usable names. Keep the latter unknown, or explicitly collect and represent its own metadata. Do not inherit the parent's name merely because the child's name cannot be read. Add a focused regression for this case.

## Other checks and limits

- All six current file hashes matched the supplied SHA manifest before review. The inherited-license fixture also asserted the pinned helper SHA before executing it.
- Gson resolves to 2.8.9 and all three Bouncy Castle jdk15to18 modules resolve to 1.86 in the supplied actual runtime report. The constraints are minimum-version constraints, not strict locks or advisory suppression. Future resolved graphs still require audit.
- The Bun diff promotes the already-resolved xmldom 0.9.12 to an explicit root devDependency and keeps Expo plist's 0.8.15 separately. It does not claim the four remaining Bun findings are fixed.
- Exact cached parent coordinates, cycle/depth limits, differing cached-POM hashes, DOCTYPE rejection and parser-error handling are present. The gaps above concern model semantics after XML parsing.
- The Expo local mapping requires exact publication coordinates, the literal local repository declaration and equality with the npm package version. Duplicate ownership is marked unknown; package/config SHA evidence is retained. This is declaration provenance, not proof of an AAR's contents, dependency locking or a trust policy.
- All 13 artifacts referenced by the durable dependency summary existed and matched their recorded SHA. The inventory contains 227 direct-POM, 7 parent-POM and 14 npm-local-publication records, matching the reported totals.
- The actual report retains Commons IO 2.6 and its two OSV findings; the preflight scanner remains unavailable. No introduced code masks those findings or claims a zero-vulnerability result.
- Android minSdk remains 24 in the actual Gradle report. The JVM receipt proves three host-JVM API checks and selected class bytecode levels, not API 24/25 execution. No Android-runtime compatibility acceptance is given here. Leaving the Commons IO upgrade unresolved is explicitly recorded; silently raising minSdk or assuming Java bytecode implies Android API compatibility would be unsupported.
- Standards sources read: `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`. No additional documented-standard violation or smell finding in the six-file scope. The findings above are correctness/spec issues.
- The root owns the full verification run. This reviewer ran only two small synthetic filesystem/XML fixture programs via `fnm exec --using=default bun`, plus read-only hash and evidence checks. Temporary caches were removed; fixture inputs/results are retained beside this report.

## Pinned input hashes

Diff SHA-256: `9db676909fdb266e0e611272a8721849d932e0f9b08c7a43bae1b50d5caa0276`.

| File | SHA-256 |
| --- | --- |
| `apps/viewer-expo/android/app/build.gradle` | `654faa0502dfbf9e3670ab5c64d90580fb8a7f7c9ffd67304b41c76e55ffae8a` |
| `package.json` | `9f84e45fda5576e88ee428a27c4ba4b260cc2e2edfe179e4c8e7e020f27573fb` |
| `bun.lock` | `7a2d0dd19800586ffced906d6aa95c512f02e229b9a57c3966d1e5c6f4e1abba` |
| `tools/release-gradle.mjs` | `8abc0eed997a6faec4de79abfdefe6ba42b7b4772b8a8cacb29703eee882b52e` |
| `tools/release-gradle.test.mjs` | `77ca3f71c16c8f3f7b18aa58642fc5142436a5d102aee22ed66897bb9ecf8acc` |
| `tools/release-gradle-licenses.mjs` | `8326aa899a539de470a13bffab4e8107bb9266d30d243c70fc1b244d78d6fd80` |
