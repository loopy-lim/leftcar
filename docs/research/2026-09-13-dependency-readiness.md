# 2026-09-13 의존성 배포 준비 조사와 수정

현재 lockfile에 대한 기계 판독 결과는 [검사 기록](../superpowers/evidence/2026-09-13-dependency-readiness.json)에 있다. 이 기록의 lock SHA와 후보 manifest의 SHA를 대조해야 한다. 검사 성공·취약점 부재·배포 수락은 서로 다른 판단이다.

## 적용한 수정

| 경로 | 이전 → 현재 | 검증 |
| --- | --- | --- |
| root/레거시 Viewer의 Vitest | 2.1.9 → 4.1.11 | JS 572개, 계약 4개(전체의 부분집합), root/Viewer/Host 타입 검사 통과 |
| Vitest의 Vite/esbuild | 취약한 Vite 5.4.21/esbuild 0.21.5 경로 제거 | 기존 Host Vite 6.4.3 유지, 갱신한 lock 감사 |
| Expo plist XML 파서 | xmldom 0.8.14/0.9.11 → 0.8.15/0.9.12 | 상위 요구 범위 안의 패치, frozen install |
| Expo 도구 YAML 파서 | js-yaml 4.3.1 → 4.3.2 | 상위 요구 범위 안의 패치, frozen install |
| Gradle wrapper 배포 파일 | 9.3.1의 공식 SHA-256 추가 | 공식 다운로드와 값 대조; wrapper 버전/jar는 유지 |

Vitest는 UI 서버 관련 critical 수정만 있는 3.2.6에서 멈추지 않고 mocker 경로 탐색 수정도 포함하는 4.1.11로 옮겼다. 공식 요구 Node >=20/Vite >=6을 현재 Node 22.21.1이 충족한다. [UI 서버 advisory](https://github.com/advisories/GHSA-5xrq-8626-4rwp), [mocker advisory](https://github.com/advisories/GHSA-82fw-gwwq-j7x9), [마이그레이션 안내](https://vitest.dev/guide/migration.html)

Vitest 4에서 기본 제외 목록이 달라져 vendor의 빌드 산출물까지 발견했다. 기존 dist/cypress/cache/config 제외를 유지하고 `.worktrees`만 추가 제외했다. 실제 CLI 파일 탐색 회귀 테스트가 현재 source/contract를 포함하고 중첩 체크아웃과 빌드 산출물을 제외함을 확인한다. Viewer 제어 소켓의 ambient `Buffer` 타입 참조도 소켓 API의 실제 타입 추론으로 바꿨다. 실행 동작을 변경하지 않는 타입 수정이다.

Gradle 배포 SHA는 `b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06`이다. 의존성 artifact 전체를 검증했다는 뜻은 아니다. [Gradle 공식 checksum](https://services.gradle.org/distributions/gradle-9.3.1-bin.zip.sha256)

## 남은 취약점과 업데이트 경계

Bun 전체/production graph 모두 **4건: high 2, moderate 2, critical 0**이다. 수정 전 전체 31건(critical 1/high 19/moderate 11), production 24건이었다. `bun audit`은 findings가 있으므로 종료 코드 1이며 이를 성공으로 바꾸지 않았다. production graph에 Expo/Metro 빌드 도구도 포함되므로 앱의 원격 공격 가능성으로 곧바로 해석하지 않는다.

| 의존성 | 상위 경로와 남은 이유 | 다음 판단 |
| --- | --- | --- |
| decode-uri-component 0.2.2 | Expo Router → query-string 7.1.3의 ^0.2.2 범위. 수정 0.5.0은 범위 밖 | 상위 패키지가 호환 버전을 채택했는지 확인하고 Router/URL 동작 검증 후 이동. [advisory](https://github.com/advisories/GHSA-vcc3-ghjq-m6fr) |
| image-size 1.2.1 | Metro 0.84.4, 현재 advisory는 <=2.0.2까지 영향 | 수정 릴리스 대기 및 신뢰하지 않는 빌드 입력 취급 검토. [ICNS](https://github.com/advisories/GHSA-w3rx-r6r6-pgpr), [JXL/HEIF](https://github.com/advisories/GHSA-5p2g-fcmc-qvqq) |
| uuid 7.0.3 | Expo config plugins → xcode 3.0.1의 ^7.0.3 범위. 수정 >=11.1.1은 범위 밖 | xcode/Expo 상위 이관 확인. 현재 Android 후보와 도구별 도달성도 분리해 판단. [advisory](https://github.com/advisories/GHSA-w5hq-g745-h8pq) |

advisory ignore, 취약 버전 강제 override, 임의의 상위 major 업그레이드는 추가하지 않았다. 이 항목은 공개 배포 수락을 담당하는 사람이 도달성·완화·잔여 위험을 검토해야 하며, 현재 내부 패키지 생성과 별개로 열린다.

## Rust, Gradle, 라이선스

`cargo-audit 0.22.2`와 1,243개 advisory DB(`b50980aad8b8f14f77e25a97b32dd94bf008b0af`, 2026-09-09)로 root 138개와 Host 597개 의존성을 검사했다. 알려진 vulnerability는 각각 0개다. Host에는 7개 정보성 경고가 있다. `unic-*` 5개는 Tauri/urlpattern 경로의 유지보수 중단 경고로 macOS/Windows 모두 도달한다. `proc-macro-error` 유지보수 중단과 `glib` unsound 경고는 이번 target 분석에서 Linux 전용 경로였다. 경고를 지우거나 모든 target이 깨끗하다고 표시하지 않는다.

`bun pm licenses`가 읽은 로컬 선언 라이선스는 package/version 639쌍, UNKNOWN 0개다. 새 release preflight는 설치되지 않은 플랫폼·workspace 항목도 포함한 Bun lockfile 845개 노드를 조사하며 161개는 로컬 라이선스 metadata를 확인하지 못했다. 두 목록의 모집단은 다르다. Gradle runtime 248개 좌표 중 226개는 cached POM에 선언 라이선스가 있고 22개는 미확인이다. 이것은 메타데이터 확보 결과이며 법적 사용 승인이나 NOTICE 완료를 뜻하지 않는다. Rust metadata의 license와 target별 의존성 정보도 조사했다. 프로젝트별 license 정책 확정과 배포물 NOTICE 검토가 필요하다.

Gradle `releaseRuntimeClasspath` 관찰 보고서는 있으나 dependency lock/verification metadata와 설정된 취약점 scanner가 없다. 이 부분은 **미검증**이다. wrapper checksum 하나나 성공한 Android 빌드를 Gradle 공급망 감사 완료로 해석하지 않는다. OSV 도구 설치나 현재 내려받은 artifact 전체를 자동 신뢰하는 verification metadata 생성은 실행하지 않았다.

현재 후보의 배포 준비 검사는 내부 서명과 공개 배포 수락을 구분하고, lock digest·의존성 inventory·검사 실패/미실행 상태를 보존해야 한다. 서명·설치/업데이트·실기기 수락은 [완료 기준](../completion-and-support.md)을 따른다.

## CI Actions 출처 고정

PR #5 담당 세션이 CI 작업 종료와 소유권 충돌 부재를 명시적으로 확인한 뒤, 같은 Actions 참조 22곳을 공식 저장소에서 다시 조회한 full commit SHA로 고정했다. annotated tag는 commit으로 해소했다. version branch인 Rust toolchain action의 실제 YAML에 1.95.0이 고정돼 있음을 확인했고, ref를 원래 값으로 정규화한 전후 workflow는 YAML 구조가 동일했다. [고정한 참조 기록](../superpowers/evidence/2026-09-13-actions-pins.json), [GitHub 권고](https://docs.github.com/en/actions/reference/security/secure-use#using-third-party-actions)

이는 후속 workflow의 원격 CI 실행 결과가 아니다. PR #5에서 성공한 CI는 이전 소스의 증거로 남긴다.
