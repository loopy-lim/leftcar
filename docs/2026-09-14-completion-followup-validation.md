# 2026-09-14 추가 마무리와 30분 검사 준비

> 이 문서는 승인 전 준비 시점의 역사적 기록이다. 이후 사용자가 GitHub 공개와 기존 DP 화면 공유를 승인했고 원래 앱·ADB 사용을 지정했다. 현재 설치·수정·실기기 상태는 [원래 앱 후속 검사](2026-09-14-original-app-stream-validation.md)를 따른다. 아래 승인 대기·미설치·시험용 앱 계획을 현재 상태로 해석하지 않는다.

새 디스플레이 생성 기능은 **이미 제거된 비활성 상태를 유지**한다. 외부 도구를 통한 생성·크기 변경과 기존 화면 모드·배율·배치·활성 상태 변경도 검사에서 제외했다. 추가 의존성 수정, 독립 검토, 전체 자동 검사, 내부 Host/APK와 후보 커밋의 원격 CI를 완료했다. **30분 실기기 스트림 검사는 아직 시작하지 않았다(실제 수집 0초).**

이전 승인 대상인 가상 시험 화면은 현재 존재하지 않는다. 새 시험 대상은 기존 실제 DP 화면이며, HDP-V105와 미러링 중이다. 이 화면을 Lenovo TB710FU로 전송해도 되는지 사용자 답변을 기다린다. 오디오·원격 입력은 OFF로 준비했다. 태블릿을 사용 중이던 Yeoyu 세션과 조율했고, 사용하지 않은 기기 시간은 반환했다. Host/Viewer 설치·실행·디스플레이 스트림 캡처·화면 변경은 수행하지 않았다.

## 새 후보와 전달 파일

- 후보 commit: `996acba8a3f539ce4658d829065ea3fc744a19e3`
- 공통 build-input SHA-256: `1904d74926243c21e642dcbbc8269aaab90e3943cf4c058def05e6d699eb0b3f` (**891개 입력**)
- 일반·시험용 Host/APK 네 빌드 모두 같은 깨끗한 후보에서 생성했다. 전체 검사 전후 입력과 commit 후 입력 파일·모드·digest가 일치한다. 이 보고서를 추가하는 이후 문서 commit은 새 바이너리 후보가 아니다.

| 로컬 전달 파일 | 실제 확인과 경계 |
| --- | --- |
| [Host 0.1.2 ZIP](../../completion-followup-artifacts-20260914/Leftcar-Host-0.1.2-internal-arm64.zip) | macOS arm64, ad-hoc. 압축 왕복 후 앱 해시·strict codesign 일치. 공증·설치 미실행 |
| [Viewer 0.1.5 APK](../../completion-followup-artifacts-20260914/Leftcar-Viewer-0.1.5-internal-arm64.apk) | `leftcar.ll3.kr`, versionCode 4, arm64, 기존 debug 인증서. 내장 Hermes JS/native 검증. 설치·업데이트 미실행 |
| [Windows Host unsigned ZIP](../../completion-followup-artifacts-20260914/Leftcar-Host-Windows-x64-unsigned-ci.zip) | 같은 후보의 CI NSIS 산출물. GitHub archive digest 일치 확인. Windows 설치·실행·서명 수락 미실행 |

Host ZIP SHA는 `23fbf22101ce6a2806f527a76bf0851c7c8732789524345feb5920470ac27926`, APK SHA는 `3766f23688c8348a50fbdb8344314c188851fb9f76a774d2e5a009a6beea5b05`다. Host 앱 해시는 이전 후보와 동일하지만 이번 빌드·manifest 검증을 별도로 수행했다. 이전 APK는 새 수정의 증거로 사용하지 않는다.

[일반 Host manifest](../../completion-followup-artifacts-20260914/leftcar-host-macos-internal-xosqJp/host-macos-internal-manifest.json), [일반 Android manifest](../../completion-followup-artifacts-20260914/leftcar-android-internal-NQh8la/android-internal-manifest.json), [전달 기록](../../completion-followup-artifacts-20260914/evidence/delivery-receipt.json)에 보관 파일의 해시·버전·서명·target이 있다. 이 로컬 링크는 저장소 옆 artifact 폴더를 함께 보존해야 한다.

시험 전용 `completion-30m` 후보도 별도 보관했다. Viewer 패키지는 `leftcar.ll3.kr.benchmark.completion_30m`, Host 식별자는 `leftcar.ll3.kr.benchmark.completion-30m`이며 전용 상태 경로와 기존 DP의 정확한 sourceId를 사용한다. [시험용 manifest 검증](../../completion-followup-artifacts-20260914/evidence/benchmark-package-verification.json), [수집 준비 context](../../completion-followup-artifacts-20260914/runtime/run-context.template.json). 아직 Host PID를 넣지 않았으므로 이 template으로 실제 수집을 시작할 수 없다.

## 수정과 검증

| 범위 | 결과 |
| --- | --- |
| 화면 생성 제외 | 런타임 생성 경로가 없음을 확인. 기존 화면만 쓰도록 지원 문서·baseline 실행 안내 수정. 관련 Viewer 테스트 40개 통과 |
| Maven 의존성 | Gson 2.8.9, Bouncy Castle jdk15to18 세 모듈 1.86. 별도 공개 Maven 234개 감사에서 8건 → **2건**. 로컬 Expo 14개는 이 취약점 조회에서 제외 |
| 라이선스 출처 | 실제 Gradle 248개 선언 확인, 미확인 **0**. 직접 POM 227·부모 POM 7·Expo 발행 정보 14. 사용 정책 승인·NOTICE 완료와 구분 |
| 독립 검토 | POM 중복 단일 필드와 잘못된 자식 라이선스 상속 2건 수정. 수정 전 13개 실패 → 관련 35개 통과. 원래 재현 6개 재검토 통과, 미해결 Critical/Important 0 |
| 전체 자동 검사 | `fnm exec --using=default bun run verify all` 종료 0. React Doctor **100/100**, JS **613개**, Rust workspace **416개**(수동 1개 ignored), Host **256 + 12개**, 타입·구조·fmt·Clippy·브라우저·Swift 검사 통과 |
| Android JVM | 19개 suite / 118개, 실패·오류·skip 0. 현재 전체 검사의 Gradle 명령 통과; JVM task는 동일 입력의 기존 결과를 UP-TO-DATE로 재사용 |
| 실제 APK | 일반·시험용 각각 최소 API 24, SecureStore/legacy preference의 full/cloud/device-transfer 제외, 불필요한 overlay/legacy storage 권한 부재를 compiled manifest/XML로 확인 |

[원격 CI 34766688291](https://github.com/loopy-lim/leftcar/actions/runs/34766688291)은 위 후보 commit에서 성공했다. macOS/Rust·TypeScript·Android·Windows Host 검사 및 NSIS 빌드가 통과했다. `Device evidence (E5-E7) — pending` job의 성공은 안내 출력의 성공이며 기기 수락이 아니다. [초안 PR #6](https://github.com/loopy-lim/leftcar/pull/6)의 이후 문서 commit CI는 해당 PR의 최신 결과를 따른다. 새 상세 보고서의 공개 push는 내부 경로·기기·검증 정보의 외부 공개 승인 부족으로 자동 승인 검토에서 거절돼 로컬에만 보관했다. 코드 후보의 초안 PR/CI는 위 결과를 유지하며, main 병합과 공개 릴리스는 수행하지 않았다.

[기계 판독 통합 기록](superpowers/evidence/2026-09-14-completion-followup.json), [전체 검사 로그](../../completion-followup-artifacts-20260914/evidence/full-verify-postfix.log), [독립 재검토](../../completion-followup-artifacts-20260914/evidence/dependency-fix-review.md), [의존성 조사](research/2026-09-14-dependency-readiness.md), [라이선스 수정 근거](research/2026-09-14-dependency-license-review-fix.md)를 연결했다. 감사 원본의 공백·해시는 바꾸지 않았다.

## 남은 실행

화면 공유 답변과 새 기기 사용 구간이 확보되면 정확한 시험 후보를 정상 UI로 페어링하고 해당 DP만 승인한다. 실제 변화와 decoder/render 증가를 먼저 확인하고 세 번의 짧은 시작/종료, 60초 warm-up 후 **1800초**를 수집한다. 준비 시간·시험 패턴 표시 시간은 30분 수집에 포함하지 않는다. 입력·오디오·디스플레이 모드를 바꾸지 않으며 실제 협상한 codec·인코딩 크기·전송 경로와 중단·자원·열 상태를 기록한다.

남은 의존성은 Bun 4건과 Commons IO 2건이다. Commons IO를 안전하게 올리려면 API 24/25 파일 작업 호환성 검증이 필요하다. Gradle lock/신뢰 정책·상시 scanner, 라이선스 정책/NOTICE, 생산용 서명·공증, 물리 입력/오디오/USB/XR/Windows 수락은 별도다. 새 manifest는 `internalBuildable=true`, `distributionReady=false`다. 기존 ARCore R8 경고 2개와 Actions Node 20 deprecation 안내도 보존했다.

원래 checkout의 main `5a8dde4`와 미추적 조사 문서 두 개, 다른 worktree를 변경하지 않았다. 기기 앱·데이터·설정·기존 reverse 8877을 조작하지 않았다. 가상 화면 생성/변경이나 화면 멈춤 재현 실험은 수행하지 않았다.
