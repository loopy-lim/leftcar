# Leftcar Public 전환 준비 설계

날짜: 2026-08-20
상태: 승인됨 (사용자: 라이선스 MIT, QR 페어링, 재시작 안전 토큰, 미디어 출발지 검사 확정)

## 배경

저장소 품질 평가 결과 코드 품질은 공개 수준이나 public 전환에 3가지 블로커가 있다:

1. git 히스토리에 `apps/host-desktop/src-tauri/target/` 빌드 산출물 31GB 잔존 — push 불가
2. LICENSE 파일 부재 (`Cargo.toml`만 `license = "Proprietary"`)
3. TCP 컨트롤 서버가 `0.0.0.0:7777`에 인증 없이 바인딩 (`apps/host-desktop/src-tauri/src/lib.rs:113`), mDNS로 LAN 광고 — LAN 내 임의 클라이언트가 화면 캡처 구동 가능

사용자 결정: 히스토리에서 제거, MIT 라이선스, QR 페어링 인증 도구 구현, 나머지 위생 정리.

## 전체 순서

코드 작업을 모두 끝낸 뒤 마지막에 한 번만 히스토리를 재작성한다 (중간에 하면 이후 커밋이 또 쌓여 두 번 일하게 됨).

1. 위생 정리 + MIT 라이선스
2. QR 페어링 구현
3. 미디어 출발지 검사
4. CI/문서 갱신
5. `git filter-repo` 히스토리 재작성
6. force push + 기존 pre-release 정리
7. GitHub public 전환 (사용자가 수행)

## 1. 라이선스 (MIT)

- 루트 `LICENSE` 파일: Copyright (c) 2026 loopy-lim
- `Cargo.toml` (workspace): `license = "MIT"`, `apps/host-desktop/src-tauri/Cargo.toml`도 동일
- 루트/앱 `package.json`에 `"license": "MIT"`

## 2. QR 페어링 (session crate → 실제 서버 연결)

### 프로토콜

`crates/session`의 `PairingService`를 그대로 사용한다:

| 단계 | 동작 |
|---|---|
| ① | 호스트 트레이 메뉴 "기기 페어링…" → 페어링 창에서 QR + 6자리 확인 코드(화면 표시, QR에 미포함) + 2분 카운트다운 표시 |
| ② | QR 페이로드: `{"v":1,"id":"offer-…","s":"<base64url 32B 시크릿>","h":"192.168.x.x","p":7777}` — QR 사진만으로는 페어링 불가 (docs/07 T-02 방어) |
| ③ | 뷰어 스캔(expo-camera) → 호스트 화면의 확인 코드 직접 입력 → `pair` 명령 전송 (offerId + secretProof + code + deviceName) |
| ④ | 호스트 `approve()` (상수시간 비교, 단일 사용, TTL 120초) 성공 → 32B 랜덤 토큰 발급 |
| ⑤ | 이후 모든 명령 엔벨로프에 `"token"` 필드 필수. 없거나 무효면 `unauthorized` 응답 |

### 재시작 안전 토큰

- 토큰은 호스트가 `{app_data}/paired_devices.json` (0600) 에 저장 — 호스트 재시작 후에도 자동 재인증
- 뷰어는 expo-secure-store에 토큰+deviceId 저장, `unauthorized` 응답 시 토큰 폐기 후 재페어링 안내

### 강화

- pair 실패 3회 → 오퍼 소각
- 승인 없는 명령 → 에러 응답 후 연결 종료
- `startStream`의 `viewer_ips` 리다이렉션 제거 (항시 요청 연결의 peer IP로만 푸시) — 임의 IP 푸시 구멍 차단

### 호스트 UI

페어링 창에 "페어링된 기기" 목록 + 개별 revoke 버튼 포함 (별도 창 없이 한 창에).

### 솔직한 한계 명시

제어 평면이 평문 TCP라 pair 시그니처를 스니핑하면 TTL 내 경합이 가능하다. 단일 사용 + 확인 코드 + 3회 소각으로 완화하며, 잔여 리스크는 docs/07과 README에 명시 (TLS/PAKE는 후속 과제).

## 3. 미디어 평면 출발지 검사

`native/android-viewer/src/jni.rs`의 수신 리스너(현재 `0.0.0.0:{port}` 무차원 수락)에 기대 호스트 IP 허용목록 추가:

- 뷰어가 제어 연결에 사용한 호스트 IP를 네이티브에 전달
- 그 IP의 연결만 수락, 위조 영상 푸시 차단
- Swift shim 무변경

## 4. 위생 정리

- untrack: `debug.keystore`, `build/apk/*.txt` + gitignore 패턴 추가
- `.cargo/config.toml` 절대 경로 → `$ANDROID_NDK_HOME` 등 환경변수
- `apps/*/package-lock.json` 제거로 pnpm 단일화
- `control.ts:83` 죽은 `queue` 변수 제거
- CI: typecheck 잡 추가, `architecture-check`가 viewer-expo도 검사 (위반 항목 수정)
- `docs/README.md` "구현 상태: 시작 전" → 현재 상태로 갱신
- README: 빌드 방법(의존성 포함), DESIGN.md 링크, 보안 범위 명시 / CONTRIBUTING.md 추가
- viewer-expo 신규 인증 로직에 대한 vitest 최소 단위 테스트

## 5. 히스토리 재작성 → 공개

1. 작업 완료 후 로컬 백업(`git bundle`) 생성
2. `git filter-repo --path apps/host-desktop/src-tauri/target --invert-paths` (31GB 제거)
3. 검증: `git count-objects -vH`, 전체 테스트 재통과 확인
4. force push (branch+tag). 기존 v0.1.0 pre-release 2건은 재작성된 커미과 불일치하므로 삭제 후 인증 포함 v0.1.1로 재릴리스 권장
5. GitHub Settings → public 전환 (사용자가 수행)

## 6. 테스트

- Rust: 페어링 모듈 단위 테스트(성공/잘못된 시크릿/만료/재사용/재시작 지속성/무토큰 거부/3회 소각) + `control_e2e.rs` 인증 시나리오 포함 갱신
- 미디어: 출발지 불일치 연결 거부 테스트
- 실기기: QR 페어링 → 스트리밍 → 호스트 재시작 자동 재인증 → revoke 확인 (EVIDENCE.md E11로 기록)
