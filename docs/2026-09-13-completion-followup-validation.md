# 2026-09-13 마무리 후속 작업 검증

후속 수정·새 패키지·원격 CI와 요청된 30분 검사의 현재 상태는 [2026-09-14 기록](2026-09-14-completion-followup-validation.md)을 따른다. 아래 내용은 이전 후보의 날짜별 검증 기록이다.

로컬 구현·독립 검토·전체 자동 검사·내부 Host/APK 준비를 완료했다. **공개 배포 준비와 실기기 수용은 미완료**다. 실기기 검사는 사용자가 추후 직접 수행하기로 한 결정을 유지한다.

## 바로 사용할 내부 후보

| 파일 | 버전 / 대상 | 서명·검증 |
| --- | --- | --- |
| [Host ZIP](../../completion-followup-artifacts-20260913/Leftcar-Host-0.1.2-internal-arm64.zip) | 0.1.2 / macOS arm64 | ad-hoc, 공증 없음. ZIP을 다시 풀어 원본 앱 해시와 strict codesign 검증 확인 |
| [Viewer APK](../../completion-followup-artifacts-20260913/Leftcar-Viewer-0.1.5-internal-arm64.apk) | 0.1.5 / versionCode 4 / Android arm64 | 내장 Hermes JS + native, 기존 Android Debug 키. 설치·업데이트 미실행 |

- 후보 코드 commit: `fdfbf7e7c0e12964a5f3c1b91fb7b617c869a13f`
- 공통 build-input SHA-256: `d51d64d54053ac0e1aa0c7d47c89b8f3815288ce2669cc262a835d2312bce248` (890개 입력)
- Host ZIP SHA-256: `5a7138a573dc671afd4297967f6513e46eebd525341de18075893aef8584c3c1`
- Host 앱 디렉터리 SHA-256: `95690d34c63a197adda7218a2e88490e0b90822318048534269ecc9997272525`
- APK SHA-256: `93cf61c743d539367c9178ab42e9cd4ed0c98dc7bff58b6a2b761c8119519e8d` (26,080,024바이트)

두 빌드는 깨끗한 같은 commit과 같은 입력 digest에서 생성했다. [Host manifest](../../completion-followup-artifacts-20260913/leftcar-host-macos-internal-exwSJd/host-macos-internal-manifest.json)와 [Android manifest](../../completion-followup-artifacts-20260913/leftcar-android-internal-D49wkS/android-internal-manifest.json)를 실제 보관 파일에 대조했고, [source snapshot](../../completion-followup-artifacts-20260913/source-snapshot.json)과도 공통 validator로 일치를 확인했다. 이 문서를 추가하는 이후 문서 전용 commit은 새 바이너리 후보가 아니다. 측정에는 위 보관 source snapshot과 두 build manifest를 사용한다.

[기계 판독 검증 기록](superpowers/evidence/2026-09-13-completion-followup.json)은 검사·검토·manifest·파일 해시와 남은 경계를 연결한다. 각 build manifest가 산출물 검증의 기준이며 이 문서는 그 결과의 인덱스다.

## 작업 분리와 로컬 커밋

열린 세션의 소유권을 확인하고 별도 `codex/completion-followup` worktree에서 작업했다. 다른 세션의 PR #5는 main `5a8dde4`로 병합됐고 CI `34758245279`가 통과했다. 그 병합본을 기준으로 가져온 뒤 workflow 소유권 해제를 확인하고 같은 Actions 참조만 SHA로 고정했다.

후속 로컬 커밋은 Viewer 설정 `c2956ba`, AOAP `c94594a`, 개인정보/Android `bf07088`, 릴리스 도구 `44d66cd`, 범위·인계 문서 `fdfbf7e`다. 원래 checkout과 다른 worktree, 원래 미추적 조사 문서 두 개를 보존했다. 원격 main이 여전히 `5a8dde4`이며 최신 공개 릴리스가 Viewer APK만 있는 v0.1.4임을 마지막에 다시 읽어 확인했다.

이 후속 브랜치는 push·새 PR·merge·원격 CI를 아직 실행하지 않았다. PR #5의 CI를 새 변경에 재사용하지 않는다.

## 실제 수정과 검토

| 영역 | 실제 변경 | 검토·수용 경계 |
| --- | --- | --- |
| Viewer 설정 | 없는 값과 읽기 실패 구분, 기본값 덮어쓰기 방지, 저장 실패·재시도 UI, 저장소/키별 순서 보장 | 전체 검토에서 화면 종료 중 마지막 변경 누락을 추가 발견해 수정. 완료된 하나의 큐 작업이 최신 변경까지 처리한 뒤 다음 화면 읽기에 양도 |
| AOAP proxy | 원래 accessory/세대별 worker 소유권, 제한 시간 종료·join, 늦은 종료 중 교체 거부, partial write·실패·panic 정리 | 39개 AOAP 회귀, 보강 테스트 200회 반복 및 mutation RED. 물리 USB 영상·케이블 수용은 별도 |
| Host 감사 로그 | 이벤트·타입 allowlist, 실행별 기기 pseudonym, 1 KiB 레코드, 5 MiB/한 백업, Unix 0600, 동시 회전·실패 처리 | 현재 내용부터 적용. 기존 원본 로그 내용은 자동 수정·삭제하지 않음 |
| Android 백업·권한 | SecureStore를 full/cloud/device-transfer에서 제외, Expo의 규칙 소유권 명시, release overlay/storage 권한 제거 | source뿐 아니라 실제 APK manifest/resource table/compiled XML까지 확인. 복원·SAF·업데이트는 기기 검사 |
| 배포 도구 | 기존 schema 2에 releaseInputs 추가, 버전·ABI·shim·lock·checksum·서명 검사, collector source binding | 빠지거나 섞인 source 기록은 raw 파일·측정 시작 전에 거부. 과거 schema 1/2 검증 지원, 내부 가능과 공개 준비 구분 |
| 의존성·문서 | Vitest 4.1.11 및 호환 XML/YAML 수정, 중첩 worktree 테스트 제외, Gradle checksum·22 Actions SHA 고정, 현재 지원/목표/역사 분리 | 정책·의존성 경고를 숨기거나 예전 패키지 결과를 새 후보로 승격하지 않음 |

각 작업 검토와 전체 62파일 검토를 수행했다. 마지막 Important 설정 누락은 3개 실패 재현 후 65개 관련 테스트로 수정하고 [재검토](../../completion-followup-artifacts-20260913/evidence/reviews/final-fix-1-review.md)에서 해결을 확인했다. 미해결 Critical/Important 지적은 없다. `release-inputs.mjs`의 긴 validation 행을 정리하는 Minor 가독성 제안만 후속 정리로 남겼다.

## 최종 자동 검사

명령: `fnm exec --using=default bun run verify all`, 종료 코드 0. [전체 로그](../../completion-followup-artifacts-20260913/evidence/final-verify-all-postfix.log).

| 검사 | 실제 결과 |
| --- | --- |
| React Doctor | **100 / 100** |
| TypeScript / 기본 JS | root·Viewer·Host 타입 통과, **61개 파일 / 596개 테스트** |
| 계약 / 브라우저 UI | 계약 4개는 위 596개에 포함; 브라우저 UI **78개 assertion** 통과 |
| Rust workspace | **416 통과**, 실패 0; 수동 vector printer 1개는 의도적 ignored |
| Host Rust | **256개 + E2E 12개** 통과 |
| 구조 / 스타일 | TS·Kotlin·Rust 구조 검사, root·Host fmt와 locked all-target Clippy 통과 |
| Swift | 실제 shim과 어댑터 테스트 바이너리 컴파일. 순수 split 정책 10개 및 retransmit 정책 실행 통과. 하드웨어 adapter 실행 미검증 |
| Android JVM | 19개 suite / **118개**, 실패·오류·skip 0. 수정 전 전체 실행 후 JVM 입력이 같아 마지막 Gradle 검사에서는 test task가 UP-TO-DATE |

전체 검사는 미커밋 snapshot에서 실행했다. 검사 전후 commit과 입력 digest가 같았고, 로컬 커밋 정리 뒤에도 전체 입력 파일·모드·digest가 같음을 확인한 다음 깨끗한 commit에서 패키지를 만들었다. 검사와 commit의 연결 근거를 JSON에 남겼다. 패키지 생성 이후에는 이 결과 기록만 갱신한다.

## 패키지 직접 검사

Host는 arm64 Mach-O, 실제 Info.plist 식별자/버전, bundled shim의 동일 바이트·필수 export와 ad-hoc seal을 확인했다. 내장 shim SHA-256은 `357ac92fe8648c4312b9e26ea1e42830b8f0c6fbcc3dac370747953fc6a66509`다.

APK는 arm64 ELF, Cargo 출력→Gradle 복사와 AGP strip→APK 내장 native 일치, 내장 Hermes JS를 확인했다. 최종 APK의 full backup·cloud backup·device transfer 모두 SecureStore와 legacy preference를 제외하며, `READ_EXTERNAL_STORAGE`, `WRITE_EXTERNAL_STORAGE`, `SYSTEM_ALERT_WINDOW`는 없다. 최적화로 바뀐 XML 경로를 resource table에서 찾아 실제 manifest 참조와 대조했다.

APK 인증서 SHA-256은 `dd0f47dc0791450eac89ac2d304930d642ff32ad888a618a1992a6225f3b9655`이며 확인한 공개 v0.1.4 APK와 같다. 파일 metadata상 비교이며 실제 덮어쓰기 업데이트를 시험한 것은 아니다. [APK 검사 기록](../../completion-followup-artifacts-20260913/evidence/android-package/inspection.json).

## 의존성과 남은 경계

두 최종 manifest 모두 `internalBuildable=true`, `distributionReady=false`다. Bun 경고는 31건에서 **4건(High 2, Moderate 2)**으로 줄었고 Critical은 0이다. Cargo audit는 root/Host lock 모두 취약점 0건이지만 Host informational 경고 7건을 보존한다. Gradle은 실제 오프라인 runtime 목록만 수집했으며 잠금·무결성 정책과 취약점 scanner는 미완료다.

라이선스의 모집단도 다르다. 로컬 `bun pm licenses`는 639 package/version 쌍에서 UNKNOWN 0이었지만, 설치하지 않은 플랫폼 항목까지 포함하는 lockfile 845개 노드는 161개가 metadata 미확인이다. Gradle 248개 좌표 중 선언 라이선스는 226개, 미확인 22개다. 이것은 배포 승인이나 법적 검토가 아니다. [의존성 상세](research/2026-09-13-dependency-readiness.md).

- 사용자가 선택한 정확한 후보로 LAN 실제 영상 3회 → 10분 → **30분**을 수행한다. 4K60·광학 지연·XR 네 창은 기존 수치와 별도 실측 기준을 유지한다.
- 입력 OFF/허용/전체 해제, TCC·승인 철회·복구, 설정·정상 업데이트·백업 복원, USB 케이블·XR·Windows 실제 동작은 미실행이다.
- Host 파일 identity는 현재 위협 모델을 유지했다. 기존 로그 내용, 수동 진단 공유와 FLAG_SECURE/XR 호환성은 문서의 제한을 따른다.
- 내부 Android debug 서명, macOS ad-hoc seal은 배포 서명·공증·업데이트 성공이 아니다. 공개 배포 절차와 후속 브랜치의 원격 CI는 별도다.
- Android 준비 빌드의 기존 ARCore 1.53.0 R8 stack-map 경고 2개와 Gradle/의존성 deprecation 경고를 보존했다. 원본 의존성이나 generated bytecode를 수정해 숨기지 않았다.

[사용자 기기 검사 순서](completion-and-support.md), [버전·업데이트 안내](versioning.md)를 따라 이어간다. 앱 설치·실행, 개인 화면/오디오 캡처, 입력 활성화, 네트워크 매핑, 키 교체 또는 공개 게시를 이번 작업에서 수행하지 않았다.

## 진행 중 확정한 판단과 비용

실행 ledger의 판단을 발생 순서대로 남긴다. 검토와 원시 ledger는 산출물 폴더의 `evidence/reviews/`에 보관한다.

| 판단 | 비용·잘못됐을 때의 영향 |
| --- | --- |
| 별도 브랜치에서 PR #5 소유권과 조율 | main이 더 바뀌면 추가 통합·재검증이 필요 |
| 기존 사용자 결정에 따라 기기 검사를 뒤로 두고 코드·패키지까지 진행 | 실제 기기 수용은 열린 상태 |
| Host identity 파일 보관을 유지하고 FLAG_SECURE는 XR 검증 뒤 결정 | OS 키 저장소 이전·스크린샷 차단은 제공하지 않음 |
| 최신 사용자 결정인 30분을 적용하고 60분 collector 확장은 취소 | 60분 안정성을 주장할 수 없음 |
| 호환 XML/YAML 패치와 Vitest 4.1.11로 취약 경로 처리 | 테스트 도구 major 변경 비용이 있어 전체 JS·타입·UI 검사를 다시 수행 |
| 감사 로그 pseudonym 정책에 맞춰 오래된 control 테스트 기대값만 수정 | 원문 device ID를 전제하는 진단 소비자는 새 기록 형식을 따라야 함 |
| 산출물을 소스 worktree 밖의 새 전용 폴더에 보관 | 후보와 영수증 폴더를 함께 보존해야 함 |
| release-inputs.mjs 긴 validation 행의 Minor 정리는 후속으로 남김 | 동작상 미해결 지적은 없으나 가독성 부담은 남음 |
