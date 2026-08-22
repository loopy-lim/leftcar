# 보안과 개인정보 보호

문서 상태: 제안안 0.1  
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
| Host/Viewer private key | 매우 높음 | OS secure storage, export 금지 |
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
  └─ authenticated control channel with explicit remaining plaintext transport risk

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
| T-02 | QR을 촬영한 제3자가 재사용 | 128-bit 이상 single-use secret, 2분 expiry, Host confirmation |
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

v0.1.1 범위에서는 LAN 내 짧은 범위 사용을 가정하며 인증 제어는 TCP로 운용한다. 미디어는 제어 peer와 같은 사설 LAN 후보 중 난수 UDP 왕복을 증명한 주소에만 전송하지만 내용 자체는 평문이다.
공개 인터넷 노출이 필요한 경우 TLS + PAKE + certificate/pinning 기반의 추가 상호인증/암호화가 요구된다.

## 6. 장치 identity

각 설치는 long-term device key pair를 생성한다.

요구:

- cryptographically secure RNG
- private key export 금지
- Host는 macOS Keychain 또는 Windows 보호 저장소 사용
- Viewer는 Android Keystore-backed 저장 사용
- TypeScript에는 public fingerprint와 opaque key handle만 전달
- backup/restore로 같은 identity를 복제하지 않음
- 앱 데이터 삭제 후 새 identity 생성

알고리즘은 transport library 지원과 security review에서 확정한다. 자체 cryptographic primitive를 만들지 않는다.

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
human_verification_code
```

QR 전체를 log, analytics, crash report에 넣지 않는다.

### 7.2 흐름

1. Host가 ephemeral secret을 생성하고 2분 expiry를 설정한다.
2. Viewer가 QR을 locally parse하고 expiry를 확인한다.
3. Viewer가 제시된 address 중 직접 연결한다.
4. secure handshake가 QR의 Host fingerprint와 일치하는지 확인한다.
5. Viewer가 자신의 public identity와 offer proof를 보낸다.
6. Host UI가 Viewer display name, fingerprint short code를 보여 준다.
7. 사용자가 승인한다.
8. 양쪽이 서로의 public identity를 저장한다.
9. offer secret과 ephemeral state를 폐기한다.
10. Viewer가 새 장기 credential로 session을 다시 인증한다.

### 7.3 규칙

- QR scan만으로 무인 승인하지 않는다.
- offer는 single use다.
- 같은 offer의 concurrent request는 최대 하나만 승인한다.
- expiry 판단은 wall clock 변경에 취약하지 않게 monotonic deadline도 함께 사용한다.
- 짧은 human code만 인증 secret으로 사용하지 않는다.
- pairing 중 Host identity mismatch는 override 버튼 없이 실패한다.

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

존재하지 않는 capability:

```text
read_clipboard
write_clipboard
read_file
record_stream
```

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
- clipboard/screenshot share action을 제공하지 않는다.
- Android recent task preview에 원격 화면이 노출될 수 있으므로 secure flag 정책을 검토한다.

`FLAG_SECURE`를 사용하면 screenshot/task preview를 막을 수 있지만 Home Space compositor/Surface 동작에 영향을 줄 수 있다. Phase 1에서 보안과 호환성을 함께 검증하고 결정한다.

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

export 전에 automatic redaction test와 사용자 preview를 제공한다.

## 17. 의존성과 release

- Cargo/pnpm/Gradle dependency pin/lock
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
- [ ] transport 암호화와 identity binding 확인
- [ ] protocol/packet fuzz 결과 보관
- [ ] JNI/unsafe review 완료
- [ ] diagnostics redaction test 완료
- [ ] Android exported component 최소화
- [ ] dependency audit/SBOM 완료
- [ ] debug secret/frame dump 제거
- [ ] revoke/stop all 실기기 확인
- [ ] protected content 비우회 확인
