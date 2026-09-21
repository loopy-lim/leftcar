# 버전·호환성·업데이트 안내

기준일: 2026-09-13. 버전 문자열은 역할이 다르다. Android·Expo·Host·개발 package의 숫자가 같다는 사실만으로 호환성을 증명하지 않으며, 같은 소스에서 생성한 패키지와 계약·native ABI를 함께 확인한다.

| 위치 | 현재 값 | 역할 |
| --- | --- | --- |
| `apps/viewer-expo/android/app/build.gradle` `versionName` | `0.1.5` | Viewer Android 릴리스 기준 |
| `apps/viewer-expo/android/app/build.gradle` `versionCode` | `4` | Viewer Android 설치 판별 값 |
| `apps/viewer-expo/app.config.ts` `version` | `0.1.2` | Expo 앱 표시 버전 |
| `apps/viewer-expo/package.json` `version` | `0.1.0` | 개발 트래킹용 |
| `apps/host-desktop/src-tauri/Cargo.toml` `version` | `0.1.2` | macOS/Windows Host 빌드 버전 |
| `apps/host-desktop/src-tauri/tauri.conf.json` `version` | `0.1.2` | 실제 Host 번들 버전, Cargo 버전과 일치 확인 |
| Host capture shim 시작 ABI | `leftcar_capture_start_v9` | 현재 Host가 요구하는 capture entry point |

## 공개 파일과 개발 후보

현재 공개 [v0.1.4](https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4)는 Viewer APK 하나만 포함한다. 이 릴리스에 현재 Host가 포함됐거나 현재 개발 Host와의 조합이 수용됐다는 뜻은 아니다. 현재 Android 소스의 `0.1.5`는 아직 이 작업에서 공개하지 않은 후보 값이다.

공개 APK를 내려받아 `aapt2 dump badging`과 `apksigner verify --print-certs`로 확인한 값은 package `leftcar.ll3.kr`, versionCode `3`, versionName `0.1.4`, 인증서 DN `CN=Android Debug, O=Android, C=US`다. APK SHA-256은 `3dade6eb378b54a60ca674335dd8d260fe5ecc8b0195ce9ba238dd99939e96fe`, 공개 인증서 SHA-256은 `dd0f47dc0791450eac89ac2d304930d642ff32ad888a618a1992a6225f3b9655`다. 파일 검사만 수행했으며 설치·업데이트는 실행하지 않았다.

개발 후보를 시험할 때는 같은 source snapshot에서 만든 Host와 Viewer를 한 쌍으로 사용한다. build manifest의 `source.sha256`·commit, `versions`, `artifactMetadata`, `target`, `signing`과 실제 artifact 해시를 확인한다. 현재 v9 Host에 이전 shim을 섞거나, 내장 JS가 없는 debug APK를 독립 배포판으로 안내하지 않는다. Host와 Viewer의 생성 계약 해시는 서로 다른 계약이므로 서로 같게 만들지 않고 각 생성물/잠금 의존성과 대조한다.

| 산출물 | 만드는 경로 | 서명·증거 범위 |
| --- | --- | --- |
| 내부 Android | `bun run build android-internal` | release 최적화/내장 JS, 기존 debug key로 서명. 내부 시험용 |
| 개발 Android | `bun run build android-debug` | 개발 서버 의존 여부를 manifest에서 확인. 독립 공개판 아님 |
| 배포 키 Android | `bun run build android-release` | 별도 키 설정 필요. 서명 성공만으로 기존 설치 업데이트·실기기 수용·게시 완료 아님 |
| 내부 macOS | `bun run build host-macos-internal` | 실제 seal 확인, 기본 ad-hoc. 공증이나 게시를 수행하지 않음 |
| Windows | Windows CI/Windows의 Tauri build | 실제 산출물/서명·GPU·설치·업데이트 근거를 별도 보관 |

빌드 결과의 표시 버전뿐 아니라 실제 APK/Info.plist의 식별자·버전, 서명 인증서·seal, ABI, 내장 JS/native를 검증한다. 내부 서명 키는 새로 만들지 않고 기존 시험 키를 명시해 사용하며 비밀 값은 로그·receipt에 기록하지 않는다.

## 빌드 전 검사와 소스 일치

```text
bun run release:preflight -- --scope android-internal --json
bun run release:preflight -- --scope host-macos-internal --json
bun run build android-internal
bun run build host-macos-internal
bun run release:manifest -- verify /absolute/path/build-manifest.json
```

preflight는 component 버전·Android ABI/Rust target·필수 capture shim 심볼·lockfile·Gradle 배포 checksum과 의존성 관측을 검사한다. 내부 빌드가 가능하다는 `internalBuildable`과 공개 배포 가능한 `distributionReady`를 구분한다. 내부 scope는 구조가 맞으면 빌드 준비를 통과할 수 있지만, 남은 취약점·알 수 없는 라이선스·미완료 검사·서명 때문에 공개 배포 상태는 false일 수 있다. `android-release` scope는 공개 준비가 미완료이면 실패한다.

Bun/Cargo의 실제 검사 결과와 Gradle의 오프라인 `releaseRuntimeClasspath` 목록을 보관한다. Gradle 목록은 잠기지 않은 현재 캐시 관측이며 취약점 검사가 아니다. 도구·캐시가 없거나 접근에 실패하면 그 상태를 그대로 남긴다. 이미 준비한 RustSec DB를 사용하려면 `LEFTCAR_CARGO_AUDIT_DB`로 디렉터리를 지정할 수 있다. 도구가 scanner나 신뢰 metadata를 자동 설치하지 않는다.

새 build manifest의 `releaseInputs`는 이 구성과 의존성 관측을 내용 해시로 연결한다. 패키지에서는 실제 버전·target·서명 증거와 필수 bundled shim 심볼도 확인한다. 이전 schema 1/2 manifest의 artifact 검증은 계속 지원하지만, `releaseInputs` 없는 과거 파일을 새 배포 준비 검사를 통과한 것으로 승격하지 않는다. 이 해시는 변경 검출용이며 외부 서명자가 보증한 빌드 attestation이 아니다.

성능 collector는 source·Host·Android manifest의 구조와 내용 해시를 검증한 뒤 commit과 build-input digest가 모두 같은지 확인한다. 빠지거나 섞인 기록은 raw 로그 생성과 측정 시작 전에 거부한다. 기존 `conditions.sourceSha256`은 source manifest **파일**의 해시이며, build-input digest와 바꾸어 읽지 않는다. 실행 길이 상한은 현재 사용자 결정대로 1,800초다.

## 수동 업데이트

자동 updater는 현재 완료 조건으로 구현됐다고 주장하지 않는다. [GitHub Releases](https://github.com/loopy-lim/leftcar/releases)에서 검증된 파일·checksum·지원 조합을 확인하는 수동 업데이트를 기준으로 한다.

1. 릴리스의 지원 OS/기기, Host↔Viewer 조합, source commit, 파일 SHA-256과 알려진 제한을 확인한다.
2. 진행 중인 스트림과 파일 전송을 정상 종료한다. 기존 앱 데이터·페어링을 먼저 삭제하지 않는다.
3. Android는 동일 application ID와 호환되는 서명으로, `versionCode`를 올린 APK의 덮어쓰기를 시험한다. 설정·페어링·Host pin 유지와 권한 재요청을 직접 확인한다. 정상 업데이트와 백업 복원을 같은 시험으로 취급하지 않는다.
4. debug 키와 배포 키가 다르면 일반적인 덮어쓰기는 성립하지 않는다. 먼저 별도 application ID의 시험판 또는 명시적 이전 경로를 제공한다. 기존 설치를 삭제해야 하는 경우 사용자 데이터 손실·재페어링을 설명하고 사용자가 선택하도록 한다. 도구가 자동 uninstall하지 않는다.
5. macOS는 기존 앱과 새 앱의 서명 requirement를 비교한다. 달라지면 화면 기록/손쉬운 사용 재승인이 필요할 수 있다. 동일한 번들 버전만으로 TCC 권한 유지가 보장되지 않는다.
6. 첫 실행에서 Host identity 불일치·승인 재검토·저장 오류를 숨기지 않는다. 승인한 화면, 입력 OFF, 프로필/FPS/커서/오디오/클립보드를 다시 확인한다.

Android SecureStore는 앱 제거/기기 복원 이후 재사용을 보장하지 않는다. 복원된 자격으로 자동 신뢰를 가정하지 말고 다시 페어링한다. Host identity 파일의 삭제/손상/복사와 OS 보호 페어링 토큰의 차이는 [보안 문서](07-security-privacy.md)에 설명한다.

서명·업데이트 기준은 [Android 앱 서명](https://developer.android.com/studio/publish/app-signing), 설치 버전 기준은 [Android 앱 버전](https://developer.android.com/studio/publish/versioning), 복원 경계는 [Expo SecureStore](https://docs.expo.dev/versions/latest/sdk/securestore/#android-auto-backup)의 공식 지침을 따른다.

## 문제 버전 회수와 지원

문제가 확인된 파일은 릴리스 안내에서 사용 중단을 명시하고, 정확한 문제가 있는 버전·SHA-256·조합과 검증된 대체 파일을 안내한다. 이전 APK로의 downgrade나 Host 재서명을 자동 안전 복구로 취급하지 않는다. 데이터/설정 형식과 서명 호환성을 확인한 뒤 복구 절차를 제공한다.

문제 보고에는 OS·기기·codec·전송 방식·해상도/FPS·창 수·재현 단계와 source/package hash를 포함한다. QR·토큰·키·개인 화면·원본 파일명·IP가 담긴 로그를 그대로 게시하지 않는다. 공개 릴리스 서명·공증, 새 설치/업데이트와 물리 영상 수용은 [지원·완료 표](completion-and-support.md)의 별도 게이트다.
