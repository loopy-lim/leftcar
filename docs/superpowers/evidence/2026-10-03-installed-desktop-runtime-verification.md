# 기존 macOS 설치본 실행 검증

2026-10-03 KST, 사용자가 지정한 기존 `/Applications/Leftcar Host.app`을 실행해 확인했다. 별도 앱으로 진행하라는 뜻으로 잘못 이해해 임시 검증 패키지를 먼저 준비·실행 요청했으나, 사용자 정정 후 기존 설치본으로 전환했다. 임시 패키지의 UI 검증 결과는 없다. 설치, 업데이트, 권한 허용, 기기 연결, 설정 토글, 파일 전송, 페어링 생성·취소·승인·철회, 가상 디스플레이 생성·제거는 수행하지 않았다.

## 검증 대상

- 설치 경로: `/Applications/Leftcar Host.app`
- Bundle ID: `leftcar.ll3.kr`
- 버전: **0.1.10**
- 실행 파일 SHA256: `e799f515cc2833d4368b89ce814680a80cf9f7cbf532d4cb6b64e40eac395524`
- 실제 실행 PID: `95656`
- 실제 WebView URL: `tauri://localhost`
- 실제 TCP listener: `*:7777`
- 로드된 native capture shim: `/Applications/Leftcar Host.app/Contents/Resources/libleftcar_capture.dylib`

이 설치본은 현재 작업 트리의 0.1.11 수정본이 아니다. 버전만으로 특정 git 태그와 소스가 일치한다고 단정하지 않는다. 아래 결과를 최신 안전성·디자인 수정의 실제 macOS 수용성 검증으로 확대하지 않는다. 최신 수정본의 자동 검증은 [기존 기록](2026-10-03-safety-and-ux-verification.md)을 따른다.

## 실제 실행 결과

| 항목 | 결과 및 근거 |
|---|---|
| 기존 앱 시작 | 실제 native 프로세스와 대기 화면이 표시됐다. 기존 설치 경로의 실행 파일 및 capture shim을 프로세스 조회로 확인했다. |
| 기본 화면 | 앱 창만 캡처해 대기 상태, 연결 안내, 주소, 도움말·환경설정 진입점을 확인했다. 현재 설치본의 기존 디자인이 표시됐다. |
| 도움말 | 버튼으로 열리고 접근성 트리에 문제 해결 설명과 닫기 버튼이 표시됐다. Escape로 닫혔다. |
| 환경설정 | 버튼과 `⌘,` 모두 창을 열었다. 닫기 버튼에 초기 포커스가 배치됐고 Tab으로 설정 조작 영역을 이동했다. Escape로 닫혔다. |
| 고급 설정 | 클릭으로 항목을 펼치고 다시 접었다. 숫자 입력이나 설정 변경은 하지 않았다. 접근성 secondary Expand 액션은 상태를 바꾸지 않았다. |
| 미인증 TCP 상태 요청 | `getStatus`에 토큰 없이 요청했을 때 `{"ok":false,"error":"unauthorized"}` 응답 후 EOF를 확인했다. |
| 미인증 TCP 화면 목록 요청 | `getCatalog`도 같은 오류 응답 후 EOF를 확인했다. 응답 데이터나 자격 증명은 출력·저장하지 않았다. |
| 종료 | `⌘Q`로 정상 종료했다. 후속 프로세스 조회에서 기존 앱·임시 앱 프로세스가 없고 7777 listener가 닫혔다. |
| 사용자 데이터 보존 | 종료 후 기존 프로필 7개 파일의 SHA256이 실행 직전과 전부 일치했다. 설치된 실행 파일의 SHA256도 동일했다. 중간 관찰에서 `source_grants.json` 해시가 달랐지만 최종 해시는 원본과 같았다. 수동 복원이나 파일 수정은 하지 않았다. |

## 확인한 개선점과 경계

도움말과 환경설정을 Escape로 닫은 뒤 포커스가 진입 버튼으로 복귀하지 않고 WebView HTML 요소로 이동했다. 기존 설치본의 보안 설정 행은 접근성 `button`으로 표시되고, 파일 공유는 `switch`로 표시됐다. 이것들은 기존 설치본에서 관찰한 키보드·접근성 개선점이다.

현재 사용자 설정은 클립보드 공유·파일 공유·외부 접속이 켬, 프라이버시 커튼·스트리밍 배지가 끔이었다. 기존 값을 유지했고 `settings.json`의 Unix 권한은 `0600`이었다. 이것은 현재 사용자 설정의 관찰이며 안전 기본값 검증이 아니다.

`source_grants.json`의 중간 변경과 종료 후 원본 해시 복귀는 승인 journal의 `dirty` 표시 수명과 일치한다. 현재 소스와 `v0.1.10`의 `source_grants.rs`는 시작 시 `dirty = true`, 정상 종료 시 `dirty = false`로 저장한다. 이 소스 경로는 관찰 결과에 대한 설명이며 설치 바이너리의 정확한 소스 provenance를 증명하지 않는다. 미인증 두 요청은 dispatch 전에 거부돼 journal 변경 경로에 도달하지 않는다.

고급 환경설정의 추가 캡처는 빈 이미지로 반환돼 그 캡처를 시각적 정상 렌더링 근거로 사용하지 않았다. 고급 항목의 펼침·접힘은 실제 접근성 상태 변경으로 확인했다. 모든 창 크기의 레이아웃, Tab 순환 전체, 페어링·설정 저장 실패·권한 변경·스트리밍·입력·USB 복구·장시간 안정성은 이 실행에서 검증하지 않았다. 미인증 TCP 두 요청의 차단은 Tauri IPC 또는 전체 인증·암호화 경계 검증과 별개다.

## 보조 기록

- `/private/tmp/leftcar-desktop-runtime-20261003-5xyhvcdc/installed-host-process.json`
- `/private/tmp/leftcar-desktop-runtime-20261003-5xyhvcdc/installed-control-probes.json`
- `/private/tmp/leftcar-desktop-runtime-20261003-5xyhvcdc/installed-runtime-summary.json`
- 사용자 프로필의 내용 없이 파일 해시만 기록한 `installed-profile-before.json`, `installed-profile-after.json`을 같은 임시 경로에 보관했다.

이번 작업은 실행 검증과 기록만 추가했다. React 및 native 구현 소스는 변경하지 않았다.
