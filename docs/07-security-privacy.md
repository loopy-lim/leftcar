# 보안과 개인정보 보호

문서 상태: 초기 요구와 구현 기록. 2026-09-13 현재 저장·권한 경계는 §6, §11, §16 및 [완료/지원 기준](completion-and-support.md)을 따른다.

보호 대상: 실시간 화면, source metadata, 장치 identity, pairing material, 진단 정보

## 1. 보안 목표

1. 페어링되지 않은 장치는 Host 존재를 최소 정보 이상 알 수 없고 화면을 받을 수 없다.
2. 페어링된 Viewer도 Host 사용자가 승인한 source만 볼 수 있다.
3. 페어링된 장치만 제어 연결을 획득하고, 제어 채널의 토큰/승인 정보가 유효해야 한다.
4. 원격 입력은 Host가 세션별로 허용하고 macOS 손쉬운 사용 권한 또는 Windows의 UIPI 무결성 경계를 만족할 때만 동작하며, 세션 난수 검증 전에는 입력을 처리하지 않는다.
5. 창 제목, 화면 pixel, token, private key가 log/telemetry에 남지 않는다.
6. 세션 종료 또는 권한 철회가 빠르고 완전하게 반영된다.
7. malformed network/media input이 unbounded allocation, panic, double free를 유발하지 않는다.
8. Kotlin shim이나 TypeScript UI가 장기 private key를 직접 다루지 않는다.

## 2. 비목표

v1은 다음 공격 환경을 완전히 해결한다고 주장하지 않는다.

- Host OS 또는 Galaxy XR OS가 이미 탈취된 경우
- root/admin 권한 악성 코드
- 물리적으로 잠금 해제된 기기 탈취
- 악의적인 GPU/codec firmware
- 공개 인터넷 relay에 대한 DDoS
- 조직용 MDM/enterprise 정책

그러나 이런 환경에서도 secret을 불필요하게 복제하거나 영구 log에 남기지 않는다.

## 3. 자산

| 자산 | 민감도 | 저장 |
| --- | --- | --- |
| 화면 frame | 매우 높음 | 메모리와 network transit만, 기본 영구 저장 없음 |
| 창 제목/앱 이름 | 높음 | UI process memory, 사용자 승인 source metadata |
| Host 장기 Ed25519 seed | 매우 높음 | `host_identity.json`; Unix 신규 생성 0600, §6의 파일 저장 경계 |
| Viewer pairing credential / Host token | 매우 높음 | Viewer Expo SecureStore, Host 플랫폼별 token store; §6 |
| pairing offer secret | 매우 높음 | 짧은 수명 memory만 |
| paired public identity | 중간 | secure/local storage |
| source ID | 중간 | opaque local/network state |
| metric | 낮음-중간 | redacted local artifact |
| IP/MAC/network 정보 | 중간 | runtime, 기본 telemetry 금지 |

## 4. 신뢰 경계

```text
Host user approval
  ├─ screen recording permission
  ├─ accessibility input permission + per-stream opt-in
  ├─ source selection
  └─ Viewer pairing approval

Untrusted LAN
  └─ control channel: secure-channel 핸드셰이크(QR 핀 + X25519/Ed25519 +
     ChaCha20-Poly1305) 필수. 루프백 진단 경로만 평문 허용.
     미디어 경로는 별도 세션 키 AEAD로 암호화한다(§20).

Viewer app
  ├─ TypeScript UI: untrusted for secrets and media bytes
  ├─ thin Kotlin shim: platform adapter only
  └─ Rust core: session/auth/protocol/media validation

Platform codec
  └─ untrusted encoded input parser boundary
```

## 5. 위협과 대응

| ID | 위협 | 대응 |
| --- | --- | --- |
| T-01 | LAN 공격자가 Host를 발견하고 연결 | pairing 이전 최소 discovery, authenticated handshake |
| T-02 | LAN 공격자가 짧은 연결 코드를 추측하거나 재사용 | 6자리 코드는 single-use, 2분 expiry, 3회 실패 시 offer 소각, Host 주소를 사용자가 확인 |
| T-03 | MITM이 Host를 바꿈 | QR에 Host public key fingerprint binding |
| T-04 | 기존 paired Viewer 도난 | Host device list와 즉시 revoke |
| T-05 | 승인되지 않은 source 요청 | source capability와 session authorization check |
| T-06 | Viewer 또는 LAN 공격자가 입력을 무단 주입 | 페어링된 미디어 세션 난수 검증, Host 세션별 기본 거부/명시 승인, reliable sequence/ACK, 종료 시 전체 해제 |
| T-07 | 무제한 fragment로 memory exhaustion | frame/fragment/stream/session byte cap와 timeout |
| T-08 | malformed H.264로 decoder crash | codec config validation, fuzz, process watchdog, paired peer라도 제한 |
| T-09 | log에 화면/창 제목 노출 | structured allowlist metric, redaction test |
| T-10 | stale task가 이전 source 재생 | task 복원 시 재인증과 catalog revision 확인 |
| T-11 | session 종료 뒤 capture 지속 | explicit teardown ack, Host visible indicator, watchdog |
| T-12 | downgrade attack | protocol/crypto minimum, negotiated version transcript binding |
| T-13 | malicious update/dependency | lockfile, checksum, signed release, SBOM/audit |
| T-14 | protected content 우회 | blank/protected error를 정상 처리, bypass 시도 금지 |

v0.1.1 범위에서는 LAN 내 짧은 범위 사용을 가정하며 인증 제어는 TCP로 운용했다. 2026-09-09 강화(§20)로 제어 평면은 QR로 핀한 호스트 Ed25519 키에 묶인 암호 채널이 되었고, 미디어는 세션 키 AEAD로 암호화한다.
공개 인터넷 노출이 필요한 경우 TLS + PAKE + certificate/pinning 기반의 추가 상호인증/암호화가 요구된다.

## 6. 장치 identity

현재 Host는 `identity.rs`의 Ed25519 identity를 `data_dir/leftcar-host/host_identity.json`에 seed hex로 저장한다. Unix 새 파일 생성은 0600이며 Windows에서는 사용자 데이터 디렉터리의 ACL에 의존한다. 기존 파일의 과도한 권한을 고치는 별도 이관, 하드웨어 키 보관, private seed export 방지는 구현되어 있지 않다. 이 파일 저장을 Keychain 보관과 같은 보장으로 설명하지 않는다.

이 위협 모델은 OS와 로그인 계정을 신뢰한다. 같은 계정으로 실행되는 프로세스, 관리자, 백업 접근 권한자는 파일을 읽거나 복제할 수 있다. 파일을 다른 Host에 복사하면 같은 public identity도 복제되므로 일반 지원 첨부·클라우드 동기화·공유 백업에 넣지 않는다. 이번 마무리 작업은 기존 사용자 키를 이동하거나 삭제하지 않는다.

`load_or_create`는 파일 부재·손상·읽기 실패 때 새 identity를 생성하고 저장을 시도한다. 저장까지 실패하면 해당 프로세스의 새 키만 남을 수 있다. 기존 Viewer의 핀 불일치는 우회하지 않고 실패한다. 복구 시 Host 저장 경로/권한과 실제 fingerprint를 먼저 확인하고, 기존 파일을 보존한 상태에서 의도한 Host인지 대조한 뒤 다시 페어링한다. 앱 재설치만으로 사용자 데이터가 삭제된다고 가정하지 않는다.

Host **pairing token**은 이 장기 seed와 별개다. `pairing.rs`의 플랫폼 token store는 macOS Keychain / Windows Credential Manager를 사용하고, 지원되지 않는 플랫폼의 파일 저장과 레거시 token 이관을 별도로 처리한다. 이관은 저장 후 다시 읽어 동일함을 확인한 경우에만 인라인 값을 제거한다. 저장 실패를 정상 페어링으로 알리지 않는다.

Android Viewer의 pairing credential과 설정은 Expo SecureStore를 사용한다. Android Keystore로 암호화된 shared preferences이므로 복원 뒤 해독할 수 없는 항목을 백업에서 제외해야 한다. 현재 자체 XML은 `SecureStore`와 레거시 `ReactNativePreferences.xml`을 full/cloud/device-transfer에서 제외하고, Expo `configureAndroidBackup: false`로 해당 규칙의 소유권을 명시한다. 앱 삭제·데이터 초기화 후에는 재페어링이 필요하다. [Expo SecureStore 공식 안내](https://docs.expo.dev/versions/latest/sdk/securestore/#android-auto-backup)

장기 키를 OS 보호 저장소로 옮기는 설계는 별도 이관·복구 검증이 필요한 후속 항목이다. 현재 Viewer JavaScript 경로는 SecureStore에서 pairing credential을 읽어 제어 채널을 연다. 아래 native 경계의 opaque handle 전용 요구는 목표이며, 현재 모든 credential이 JavaScript에서 격리되어 있다는 증거가 아니다.

## 7. 페어링 프로토콜

### 7.1 QR payload

논리 필드:

```text
pairing_version
host_public_fingerprint
ephemeral_offer_id
single_use_random_secret
expiry
address_hints
```

6자리 확인 번호는 Host 화면에 표시하고 QR에는 포함하지 않는다. QR 스캔 경로가 기본 흐름이다 — Viewer는 QR의 secret을 제시하고 §7.2의 Host 사용자 승인으로 페어링이 완결된다. 6자리 번호는 QR을 스캔할 수 없는 기기의 직접 입력 경로다.

QR 전체를 log, analytics, crash report에 넣지 않는다.

### 7.2 흐름

1. Host가 ephemeral offer secret을 생성하고 2분 expiry를 설정한다.
2. Viewer가 QR을 locally parse하고 expiry를 확인한다.
3. Viewer가 제시된 address 중 직접 연결한다.
4. secure handshake가 QR의 Host fingerprint와 일치하는지 확인한다.
5. Viewer가 자신의 public identity와 offer secret을 제출한다(6자리 코드는 비운다).
6. 아직 승인 전이면 Host는 `{"status":"pending"}`으로 답하고, Viewer는 최대 150초까지 약 2.5초 간격으로 같은 secret으로 다시 묻는다. pending 폴링은 offer를 소각하지 않는다.
7. Host UI 승인 카드가 요청 기기 이름을 보여 주고 사용자가 [허용]/[거절]을 고른다. Host UI는 대기 목록을 약 1.5초 간격으로 갱신한다.
8. 허용 시 양쪽이 서로의 public identity를 저장하고 Host는 완료 기록을 남긴다. Viewer의 다음 폴링이 같은 secret으로 token을 픽업한다(픽업도 constant-time 비교로 secret 소유를 다시 증명한다).
9. 거절 시 offer를 소각하고 Viewer 폴러가 "pairing rejected"를 명시적으로 알린다.
10. offer secret과 ephemeral state를 폐기한다.
11. Viewer가 새 장기 credential로 session을 다시 인증한다.

6자리 직접 입력(pair_by_code)은 QR을 스캔할 수 없는 기기와 구버전 Host를 위한 폴백 경로로 남는다. 새 Viewer가 구버전 Host에 QR 승인을 시도하면 "pairing failed"로 답하고 Viewer는 PIN 입력 화면으로 폴백한다. 승인된 페어링은 기존 paired device 목록에 나타나며 revoke를 지원한다.

### 7.3 규칙

- QR 시크릿 제시만으로 승인하지 않고 Host 사용자의 명시적 허용(승인 카드)을 요구한다. 6자리 코드는 QR을 쓸 수 없는 기기의 직접 입력 경로다.
- offer는 single use다.
- pending 폴링은 offer를 소각하지 않는다. 잘못된 secret 제출은 이전과 같이 3회 실패 시 offer를 소각한다.
- token 픽업도 secret 소유 증명을 다시 요구한다(constant-time 비교).
- 거절은 offer 소각 후 명시적으로 알린다.
- 같은 offer의 concurrent request는 최대 하나만 승인한다.
- expiry 판단은 wall clock 변경에 취약하지 않게 monotonic deadline도 함께 사용한다.
- 짧은 human code만 인증 secret으로 사용하지 않는다.
- pairing 중 Host identity mismatch는 override 버튼 없이 실패한다.
- 구버전 호스트/뷰어 조합은 6자리 입력으로 폴백한다.

## 8. 세션 보안

transport 후보별 최소 요건:

- QUIC: TLS 1.3, certificate/public key pinning, mutual device authentication
- WebRTC: DTLS-SRTP와 signaling identity binding, paired device authorization

공통:

- session ID와 protocol transcript binding
- replay protection
- key rotation/reconnect
- version downgrade 방지
- source request authorization
- frame/source/session ID binding
- close/revoke 즉시 반영

network encryption은 source permission을 대체하지 않는다.

## 9. capability model

v1 device capability:

```text
view_catalog
view_source(source_id, revision, expiry)
remote_input(stream_session, host_opt_in)
```

현재 파일·클립보드 기능은 별도의 Host 토글(기본 OFF)과 인증된 명령으로 제한한다. 초기 설계의 “존재하지 않는 capability” 목록은 현재 구현과 달라 삭제했다. 화면 녹화 저장 기능(`record_stream`)은 제공하지 않는다.

source capability는 다음에 bind한다.

- paired viewer device
- Host session
- approved source ID
- source revision
- short expiry/renewal
- codec/profile upper bound

Viewer가 임의 source ID나 입력 세션 난수를 추측해도 authorization에 실패해야 한다. 입력 capability는 source 보기 승인만으로 자동 획득되지 않으며 Host 토글을 끄면 즉시 주입을 중단하고 눌린 키와 버튼을 해제한다.

Windows `SendInput`은 UIPI에 따라 Leftcar Host와 같거나 낮은 무결성 수준의 프로세스에만 입력을 주입할 수 있다. current-user 설치와 일반 권한 실행을 기본으로 하며, 관리자 앱을 제어하기 위해 Host를 자동 상승시키지 않는다. `SendInput`이 일부 이벤트만 처리하면 세션 진단에 오류를 기록하고 reliable packet은 재주입하지 않도록 ACK한다.

## 10. Host 사용자 가시성

Host는 캡처 중임을 항상 알 수 있어야 한다.

- menu bar/tray indicator
- 현재 capture source count
- paired Viewer name/count
- stop all 즉시 action
- source별 stop/revoke
- permission status

Host UI가 crash해도 capture core가 무기한 invisible 상태로 남지 않도록 watchdog 정책을 둔다. UI/core가 같은 process면 process 종료 시 capture가 종료된다. 분리 process면 heartbeat와 bounded grace period가 필요하다.

## 11. 화면 데이터 수명

- decoded/encoded frame을 파일에 쓰지 않는다.
- crash dump에 large media buffer가 들어가지 않도록 설정을 검토한다.
- buffer pool은 재사용하며 release 후 참조하지 않는다.
- debug build의 frame dump는 explicit local developer flag, synthetic source에서만 허용한다.
- 화면 screenshot/녹화 export action은 제공하지 않는다. 클립보드 텍스트·이미지 공유는 별도 opt-in 기능이며 화면 캡처 저장과 구분한다.
- Android recent task preview에 원격 화면이 노출될 수 있으므로 secure flag 정책을 검토한다.

현재 StreamActivity는 recent task에 나타나고 `FLAG_SECURE`를 설정하지 않는다. 따라서 원격 화면의 screenshot/recent preview 차단을 보장하지 않는다. Android 공식 API는 secure window의 screenshot과 비보안 display 표시를 제한한다. XR Home Space/Surface가 영향을 받는지는 이 프로젝트의 실기기 미검증 항목이다. 태블릿과 Galaxy XR 각각에서 표시·창 전환·복귀·preview 결과를 기록한 뒤 별도 변경으로 결정한다. [Android의 민감한 화면 보호](https://developer.android.com/security/fraud-prevention/activities)

## 12. metadata 최소화

source catalog에는 UI에 필요한 정보만 담는다.

필요 후보:

- 사용자 표시 이름
- 앱 이름
- source kind
- resolution/aspect ratio
- available/approved

보내지 않을 정보:

- 전체 파일 경로
- document URL
- process command line
- PID/HWND/native handle
- 다른 창 목록
- 사용자 계정명

표시 이름은 Viewer memory에 존재하지만 structured log에서는 hash/omit한다.

## 13. parser와 resource limit

초기 상한은 benchmark 뒤 조정하되 코드에 명시한다.

| 항목 | 초기 상한 가설 |
| --- | --- |
| control message | 256 KiB |
| source catalog entries | 256 |
| active source per session | 8 |
| frame encoded bytes | 16 MiB |
| fragments per frame | 16,384보다 훨씬 낮은 measured bound |
| incomplete frame per source | 2 |
| pairing attempts | exponential rate limit |
| protocol nesting/string | schema-specific cap |

상한 초과는 allocation 전에 거부한다.

## 14. Android native 경계

### TypeScript

- secret raw bytes 접근 금지
- encoded frame 접근 금지
- opaque handle만 사용
- UI 입력은 Rust command에서 재검증

### Kotlin shim

- key material 접근 금지
- network socket 생성 금지
- codec policy 금지
- Surface jobject와 Activity lifecycle만 Rust에 전달
- transport-layer TLS/PAKE 협상은 수행하지 않고, 제어 채널 토큰/역방향 피어 검증으로만 제한

### Rust/JNI

- null/invalid jobject 검증
- `ANativeWindow_acquire/release` 균형
- thread attach/detach 규칙
- JNI exception 확인/clear policy
- callback 후 dangling global ref 금지
- panic이 FFI를 넘어가지 않음

## 15. 보호 콘텐츠

운영체제나 앱이 capture를 금지한 콘텐츠는 blank, protected, permission error로 나타날 수 있다.

Leftcar는:

- 우회 API나 injection을 사용하지 않는다.
- 오류를 해당 source에만 표시한다.
- 사용자가 다른 source를 선택하도록 안내한다.
- 보호 여부를 숨기기 위해 software capture fallback을 시도하지 않는다.

## 16. 진단과 telemetry

기본은 local-only다.

diagnostic allowlist:

- app/build/OS version
- codec capability
- numeric metric
- stable error code
- lifecycle event kind
- opaque ID의 run-scoped hash

denylist:

- frame payload
- window title
- QR/pairing offer
- IP/MAC
- certificate/private key
- file path
- raw native exception message 검토 전 값

감사 로그 저장 경로에는 허용 이벤트·타입별 필드만 기록하는 회귀 검증을 적용한다. 지원용 자동 export/사용자 preview UI는 아직 제공하지 않는다. `sessions.jsonl` 외의 콘솔·빌드·성능 도구 로그까지 같은 필터가 적용된다고 가정하지 않는다. 공유 전에는 사용자가 내용을 확인해야 한다. 과거 로그는 자동으로 재작성/삭제하지 않으므로 이전 세대에는 장치 ID·IP·파일명이 남아 있을 수 있다.

기기 식별자는 프로세스마다 새로 만든 임의 키와 SHA-256으로 변환한 `dev:<24 hex>` 형태로만 남는다. 같은 Host 실행 안에서만 연결해서 볼 수 있고, 재시작하면 달라진다. 세션 ID는 숫자로 유지하며 입력 소유권 변경 목록은 최대 32개와 원래 개수를 남긴다.

보관은 시간 만료가 아닌 5 MiB 기준 회전과 한 세대 백업이다. 레코드는 줄바꿈을 제외하고 최대 1,024바이트이며, 쓰기 전 크기를 확인하므로 활성 파일은 한 레코드만큼 상한을 넘은 뒤 다음 기록에서 회전할 수 있다. 같은 프로세스의 append/회전을 직렬화하며 Unix에서 현재/회전 파일 권한을 0600으로 제한한다. 파일 시스템 오류로 보관 상한이나 권한을 지킬 수 없으면 새 기록을 중단하고 스트림은 계속한다. 이 직렬화는 별도의 프로세스 간 파일 잠금을 뜻하지 않는다. 정확한 테스트·패키지 증거는 최종 완료 기록을 따른다.

## 17. 의존성과 release

- Cargo/Bun/Gradle dependency pin/lock
- release artifact checksum
- SBOM 생성
- known vulnerability audit
- Rust `unsafe` inventory
- JNI/FFI boundary review
- Android exported component review
- signing key 분리
- debug endpoint와 frame dump flag release 제거

Rustra는 pin된 commit/tag와 contract hash를 기록한다. 로컬 개발 branch를 암묵적으로 release dependency로 사용하지 않는다.

## 18. 보안 테스트

필수 자동 테스트:

```text
unpaired_peer_cannot_list_sources
paired_peer_cannot_view_unapproved_source
expired_offer_is_rejected
replayed_offer_is_rejected
host_fingerprint_mismatch_is_fatal
revocation_closes_existing_streams
unknown_input_like_command_is_denied
native_input_requires_session_nonce_and_host_opt_in
oversized_control_message_allocates_nothing_large
fragment_flood_stays_within_memory_bound
diagnostics_redact_title_path_token_and_ip
panic_does_not_cross_jni_or_c_abi
stream_task_restore_requires_reauthentication
```

수동/통합:

- Wireshark에서 payload 평문 부재
- revoke 중 active stream 즉시 종료
- process death 후 stale task 동작
- Android recent preview 정책
- Host crash/Viewer crash 후 capture 종료
- screen recording permission 철회

## 19. 출시 보안 체크리스트

- [ ] threat model review 완료
- [ ] paired/unpaired negative test 완료
- [ ] source capability test 완료
- [ ] transport 암호화와 identity binding 확인 (제어·미디어 평면 구현 완료 — 실기기 스트리밍 검증만 남음, §20)
- [ ] protocol/packet fuzz 결과 보관
- [ ] JNI/unsafe review 완료
- [ ] diagnostics redaction test 완료
- [ ] Android exported component 최소화
- [ ] dependency audit/SBOM 완료
- [ ] debug secret/frame dump 제거
- [ ] revoke/stop all 실기기 확인
- [ ] protected content 비우회 확인

## 20. 2026-09-09 보안 강화 구현

외부 원격 제품(RustDesk·AnyDesk·TeamViewer·CRD·Parsec·Moonlight) 대비 격차 분석의 후속 구현.

### 구현 완료

- **제어 평면 세션 암호화** (`crates/secure-channel`): 호스트 장기 Ed25519
  정체키(QR v2 `k` 필드로 뷰어에 핀) + 뷰어 임시 X25519로 PFS 세션 키 합의,
  ServerHello 전사 서명으로 MITM·재생 차단, 이후 모든 줄을
  ChaCha20-Poly1305 봉인 프레임으로 교환. 루프백 진단 경로(tools, 테스트)만
  평문 유지. Rust↔TS 상호 운용은 고정 벡터로 잠김.
- **호스트 정체키 영속**: `data_dir/leftcar-host/host_identity.json`(0600).
- **:7777 무차별 백오프**: 60초 창 5회 토큰 실패 시 60초 차단(루프백 제외).
- **철회 즉시 효력**: 세션을 장치에 귀속(`authorize_device`)해 revoke 시
  라이브 스트림을 강제 종료 — §18 `revocation_closes_existing_streams` 충족.
- **토큰 저장소 분리**: macOS Keychain / Windows Credential Manager를 사용한다. 현재 운영 플랫폼에서는 OS 저장 실패를 파일 폴백으로 숨기지 않는다. 기타 플랫폼의 파일 저장과 레거시 이관은 §6을 따른다.
- **세션 감사 로그(당시 구현)**: 시작/종료·철회를 `sessions.jsonl`에 기록했다. 당시에는 장치·IP·파일명이 일부 이벤트에 포함됐다. 2026-09-13 후속의 제한 필드/식별자 정책은 §16을 따른다.
- **권한 게이트 클립보드 텍스트 동기화(기본 꺼짐 — 호스트 토글이 닫혀 있으면
  모든 클립보드 명령 거부, 256KiB 상한, 접근 기록은 감사 로그에 메타만)**
- 파일 전송 v2(호스트 게이트 기본 꺼짐, 512MiB 상한, 1MiB 청크, 암호화된 제어 채널 경유, 전송 기록은 감사 로그에 메타만). v2부터 양쪽
  모두 디스크로 스트리밍한다 — 뷰어는 범위 읽기로 청크를 올리고 받은
  청크를 append하므로 파일 크기와 무관하게 메모리는 청크 하나에 묶인다.
  실패한 전송은 sendFileCancel/fetchFileCancel로 즉시 정리하고(뷰어가
  finally에서 보낸다), 버려진 전송은 30분 만료 스윕이 지운다.
- 클립보드 동기화는 텍스트와 이미지(PNG, ≈6MiB 상한)를 지원한다 — 텍스트
  우선, 이미지 해시는 "i:"+sha256(base64)로 구분한다. 읽기·쓰기 모두
  감사 로그에 메타(바이트 수·종류)만 남는다.

### 진행·잔여

- **미디어 경로 AEAD(구현 완료, 실기기 검증 대기)**: 뷰어가 CSPRNG로 세션
  키 32B를 생성해 네이티브 prepare와 (암호화된 제어 채널의) startStream
  args로 전달한다. 방향마다 HKDF-SHA256(info `leftcar/media/v1`)로 c2s/s2c
  키를 따로 도출한다 — 원본 세션 키를 양방향에 그대로 쓰면 두 송신
  카운터가 1, 2, 3…으로 겹치는 매 프레임 (key, nonce) 재사용이 생긴다.
  카운터는 인스턴스마다 무작위 지점에서 시작해, reconfigure가 세션 키를
  재사용하며 봉인기를 재생성해도 논스가 반복되지 않는다. 호스트 셸
  macOS shim은 CryptoKit ChaChaPoly, Windows·뷰어 네이티브는
  DatagramSealer로 같은 와이어 레이아웃(`counter‖ct‖tag`)을 쓴다. 2026-09-14
  실제 앱 연결 검사에서 Swift의 잘못된 tag 위치를 발견해 이 배치로 맞췄다.
  방향 키 도출 벡터에 더해 Swift↔Rust 실제 봉인·복호화와 공통 패킷 벡터를
  검증한다. LCH1 도달성 증명도 봉인 프레임으로 대체됐고
  토큰 접미사 인증은 제거됐다(평문 미디어 경로 부재 — 구식 shim/뷰어는
  시작 거부). 키 없는 startStream은 실패한다. 세션 내 카운터는 전송 전환과
  무관하게 유지되며, split 4K 타일이 하나의 암호 인스턴스를 공유한다.
- 당시 구현의 T-05 격차: 페어링 토큰 보유자가 모든 디스플레이를 열 수 있고 OS 입력 권한이 있으면 입력이 자동 활성화됐다. 이 항목은 2026-09-09 시점의 기록이다. 개발 브랜치에서 수정한 화면·입력 승인 정책은 §21을 따른다.
- **프라이버시 세션 옵션(2026-09-10 구현, 실기기 검증 대기)**: 호스트
  토글(기본 꺼짐, 0600 settings.json)을 추가했다.
  - *프라이버시 커튼* — 스트리밍 중 모니터마다 검은 풀스크린 오버레이를
    띄운다. macOS shim이 캡처 필터에서 자기 프로세스의 "leftcar-curtain"
    창을 제외하므로 원격 뷰어는 화면을 그대로 본다(WGC 모니터 캡처는 창
    제외를 지원하지 않아 Windows v1은 no-op). 세션 시작·종료·토글에
    맞춰 자동으로 띄우고 내린다.
- **세션 절전 방지(2026-09-10)**: 캡처 세션이 살아 있는 동안 macOS는
  IOPMAssertion(시스템+디스플레이), Windows는 스레드별
  SetThreadExecutionState로 절전을 막는다.
- 유휴 타임아웃, 토큰 만료/로테이션, dylib/APK 서명(§17·§19) 미구현.
- USB AOAP 제어 채널은 물리 접근 전제로 평문 유지(v1).


## 21. 2026-09-13 개발 브랜치의 화면·입력 승인

이 절은 개선 브랜치의 실제 Host·네이티브 코드와 독립 회귀 검토를 설명한다. 기존 배포 파일의 동작이나 실기기 장시간 통과를 의미하지 않는다. 현재 제품의 승인 대상은 디스플레이이며, `host-core`의 가상 모델에 있는 앱 창 예제를 실제 창 캡처 지원으로 취급하지 않는다.

새로 페어링한 기기와 기존 승인 정보가 없는 기기는 Host에서 화면 접근을 검토해야 한다. Viewer의 카탈로그·시작·화면 변경은 인증된 기기의 승인만 사용한다. `sourceId`는 목록 순번과 구분하며 실제 캡처 생성 직전에 macOS 디스플레이 UUID 또는 Windows 모니터 경로로 다시 확인한다. 화면이 사라지거나 식별자가 모호하면 다른 화면으로 대체하지 않고 실패한다. 현재 Host는 승인 수명을 전달하는 v9 네이티브 시작 API가 필요하며, 예전 shim을 발견하면 시작을 거부한다.

화면 보기와 원격 조작은 별도 승인이다. 새 세션의 입력은 기본 꺼짐이며 OS 입력 권한이 있어도 Host가 그 세션을 켜야 한다. 같은 화면을 재구성할 때는 최신 Host 선택을 따르고, 다른 화면으로 변경하면 입력 승인을 다시 받는다. 재구성 중 입력을 끄면 교체 또는 복구된 세션도 그 차단을 따른다. 살아 있는 네이티브 세션의 입력 차단이 실패하면 승인 수명을 무효화하고 세션을 종료하며 오류를 표시한다.

화면 승인을 변경하면 해당 기기의 진행 중·대기 중 세션을 무효화하고 정리한다. 현재 정책은 승인 추가나 재검토 때도 해당 기기의 세션을 다시 열도록 하므로 잠시 연결이 끊길 수 있다. 오래된 시작·재구성 완료는 취소된 승인이나 재페어링 전 자격을 다시 사용할 수 없다. 오디오 소유권도 IP 주소 대신 Host가 인증한 자격 수명에 묶는다.

| 저장·재시작 상황 | 동작 |
| --- | --- |
| 정상 종료 후 같은 자격으로 재시작 | 안전하게 기록된 승인 유지 |
| 최초 실행, 손상·알 수 없는 저장 형식, 비정상 종료 | 저장된 선택이 있어도 자동 승인하지 않고 검토 요청 |
| 실행 중 상태를 안전하게 기록하지 못함 | 승인이나 제어 서버를 활성화하기 전에 시작 실패를 알림 |
| 승인 제거 저장 실패 | 접근 차단을 유지하고 저장 오류 표시; 이전 승인을 복구하지 않음 |
| 같은 Host 상태 폴더로 중복 실행 | 두 번째 프로세스의 프로필 소유 거부 |

프로필의 `.source-grants.lock`은 프로세스 수명 동안 유지하고 파일 자체를 삭제하지 않는다. `source_grants.json`은 승인을 활성화하기 전에 `dirty=true`로 기록한다. 정상 종료는 새 작업을 막고 이미 허용된 작업과 네이티브 정리를 마친 뒤 현재 선택을 `dirty=false`로 기록한다. 대시보드를 닫아 숨기는 것은 정상 종료가 아니다. 정리가 실패하거나 종료 기록이 불확실하면 이를 성공으로 처리하지 않는다. 이 계약은 프로세스 종료와 파일시스템 작업 결과의 범위이며 실제 하드웨어 전원 장애·임의 저장장치 되돌림을 입증하지 않는다.

권한 저장의 성공 응답은 목록 새로고침 실패와 무관하게 확인된 상태로 반영한다. 실패한 변경은 해당 기기의 오류로 유지하며, 실패를 확인한 뒤 시작한 재시도가 성공해야 다시 확인된 승인으로 표시한다. 다른 기기의 활동이나 단순 새로고침은 그 실패를 해소하지 않는다. 기기를 삭제할 때 저장 오류가 생겨도 접근은 차단하고, 해당 행이 사라진 뒤에도 오류를 표시한다. 재페어링은 새 자격으로 다루므로 늦게 도착한 이전 응답이 승인을 되살리지 못한다.

Host 내부 UI 계약의 `credentialId`·`stateRevision`은 비밀이 아닌 자격 수명과 상태 순서를 나타낸다. 기기 삭제 결과는 삭제된 기기와 저장 오류를 함께 반환한다. 이 상태를 원격 토큰이나 네이티브 승인 대신 사용하지 않는다. 실제 권한은 Rust의 인증·승인 수명과 네이티브 생성·송신·입력 경계에서 검사한다.

실제 GUI 종료와 지연된 OS 콜백, 디스플레이 연결 변경, Windows 파일시스템·입력, USB 및 장시간 스트리밍은 각각 별도 실행 증거가 필요하다. 단위·브라우저 회귀 검사와 교차 컴파일은 이 실행 검증을 대신하지 않는다.
