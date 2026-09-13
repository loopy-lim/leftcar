# 2026-09-13 마무리 후속 작업 검증

상태: 로컬 구현·검토 진행 중. 이 문서는 마지막 전체 검사와 내부 패키지 생성 이후 정확한 소스·산출물 근거로 갱신한다. 실기기 검사는 사용자가 추후 직접 수행하기로 했다.

## 작업 분리와 기준

열린 세션의 소유권을 확인한 뒤 `codex/completion-followup`과 별도 worktree를 사용했다. 앞선 PR #5는 별도 세션에서 main `5a8dde46b4841be57c881a7781050d74d4dafca0`로 병합됐고, 해당 PR의 CI `34758245279`가 통과했다. 이후 그 병합본을 이 작업의 기준으로 가져왔다. 다른 worktree와 원래 checkout의 미추적 조사 문서를 보존했다.

워크플로 소유권이 해제된 뒤 같은 Actions 참조를 commit SHA로 고정했다. 새 후속 코드의 원격 CI·push·PR·merge는 이 작업에서 아직 수행하지 않았다. 앞선 PR의 CI를 이 후속 변경의 통과 근거로 재사용하지 않는다.

## 닫은 코드 경계

| 영역 | 실제 변경 | 실행 근거와 한계 |
| --- | --- | --- |
| Viewer 설정 | 읽기 실패와 없는 값을 구분, 실패 시 기본값 덮어쓰기 방지, 저장 실패·재시도 표시, 같은 저장소의 이전 화면 작업까지 순서 보장 | 관련 59개 테스트·타입 검사·React Doctor 100/100. 실제 앱 재시작 수용은 별도 |
| AOAP proxy | 원래 accessory lease와 세대별 worker 소유권, 제한 시간 종료·join, 종료가 늦으면 대체 worker 시작 거부, partial write·실패·panic 정리 | AOAP 39개, 수정 테스트 200회 반복, identity-check mutation RED. 물리 케이블·USB 영상은 미실행 |
| Host 감사 로그 | 허용 이벤트·타입만 저장, 프로세스별 device pseudonym, 1 KiB 레코드 상한, 5 MiB/한 백업, Unix 0600, 동시 회전·실패 처리 | 최종 Host 256개 + E2E 12개, fmt·Clippy 통과. 기존 로그 내용은 자동 수정하지 않음 |
| Android 백업·권한 | SecureStore를 full/cloud/device-transfer에서 제외, Expo가 별도 규칙을 재생성하지 않도록 설정, release의 불필요한 overlay/storage 권한 차단 | 실제 release/debug merged manifest 확인. 최종 APK resource 검사는 패키지 생성 후 기록 |
| 의존성·테스트 목록 | Vitest 4.1.11, 호환 XML/YAML 패치, 중첩 worktree 및 기존 비대상 경로 제외, Gradle checksum·22 Actions SHA 고정 | 같은 실제 프로젝트 59개 파일 유지. 이 단계의 JS 572개 통과. 최종 도구 추가 이후 전체 개수는 마지막 검사 기준 |

설정·USB 독립 검토에서 발견한 저장 순서와 테스트 경쟁 조건을 수정하고 재검토했다. 감사 로그의 독립 검토도 승인됐다. 배포 도구의 독립 검토도 승인됐으며 Critical/Important 지적은 없다. 긴 validation 행을 정리하는 Minor 가독성 제안은 후속 정리로 남겼다. 전체 변경의 최종 검토 결과는 종료 시 갱신한다.

배포 도구는 새 manifest의 `releaseInputs`, 실제 package metadata·서명·shim 검사와 collector의 source binding을 연결했다. 도구 추가 후 JavaScript 전체 61개 파일/590개 테스트와 타입 검사가 통과했다. 실제 내부 preflight는 내부 준비 가능/공개 배포 불가로, 공개 Android preflight는 실패로 판정했다.

## 의존성 판정

[의존성 조사](research/2026-09-13-dependency-readiness.md)와 [기계 판독 관측](superpowers/evidence/2026-09-13-dependency-readiness.json)에 잠금 파일과 실제 도구 결과를 연결했다. Bun 전체 경고는 31건에서 4건으로 줄었고, 남은 것은 high 2건·moderate 2건이다. Critical은 0건이다. 동일한 4건이 production dependency graph에도 있으므로 개발 전용이라고 일괄 제외하지 않는다.

라이선스 목록의 모집단도 구분한다. `bun pm licenses`가 읽은 로컬 package/version 639쌍에는 UNKNOWN이 없었지만, 새 preflight는 설치되지 않은 플랫폼 항목까지 포함한 Bun lockfile 845개 노드를 조사해 161개의 metadata 미확인을 남겼다. Gradle runtime은 248개 좌표 중 226개에 선언 라이선스가 있고 22개는 미확인이다. 미확인을 무허가 또는 승인으로 해석하지 않는다.

Cargo audit는 두 lockfile에서 취약점 0건이지만 Host graph에 informational 경고 7건이 남는다. Gradle dependency 목록은 취약점 검사 결과가 아니며, 별도 scanner·dependency verification 정책은 미검증이다. 라이선스 선언 목록도 법적 검토나 배포 승인으로 표현하지 않는다.

## 마지막 검사와 패키지

전체 `bun run verify all`, 독립 전체 검토, 같은 소스의 내부 Host/APK 생성 및 manifest 검증은 진행 중이다. 이 절이 실제 명령·종료 코드·해시로 채워지기 전에는 이 문서를 최종 후보 영수증으로 사용하지 않는다.

## 남은 수용 경계

- 선택한 패키지에서 실제 LAN 영상부터 세 번 확인하고 10분, 30분으로 진행한다. 실험 중 소스·패키지·조건이 바뀌면 별도 실행이다.
- 화면 기록·접근성·화면 승인 철회, 입력 OFF와 허용 후 전체 해제, 설정·백업 복원·업데이트, USB 재연결, XR 네 창·비초점·IME·열 상태를 검사한다.
- FLAG_SECURE는 XR/Home Space 호환성이 확인되기 전까지 켜지 않았다. recents·스크린샷 보호를 보장하지 않는다.
- 내부 Android debug 서명과 macOS ad-hoc seal은 배포용 서명·공증·실제 업데이트 성공이 아니다. 공개 릴리스와 Windows 실제 GPU/설치 검증은 별도다.
- 4K60·광학 지연은 소스 테스트, 패키지 생성, 짧은 과거 관측으로 달성 처리하지 않는다.

구체적인 사용자 검사 순서는 [지원 범위와 완료 기준](completion-and-support.md), 설치와 서명 전이는 [버전·업데이트 안내](versioning.md)를 따른다.
