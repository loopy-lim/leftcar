# Leftcar 외부 접속·성능·안전성 비교

조사일: 2026-10-13. 기준 소스: `cc504306dee40b6e83570f36aec3af23d445e686`와 조사 시작 시 존재하던 Windows MTU/송신 예산 및 Android 재조립 수정. 비교·우선순위 표는 개선 전 기준이며, 같은 작업에서 실제로 바뀐 내용은 마지막 절에 분리했다. 공개 자료는 아래의 공식 문서·소스만 사용했다. 경쟁 제품을 설치해 동일 기기에서 측정한 성능 비교는 아니다.

**가장 큰 격차는 암호화나 FEC의 부재가 아니라 Windows와 macOS의 복구·적응 기능 차이, 외부망 연결 경로의 진단 부족, 현재 패키지의 실행 증거 부족이다.** 기존 README와 지원 문서는 9월 기준으로 남아 있어 현재의 Tailscale, WAN opt-in, 입력 기본 정책도 정확히 설명하지 못한다.

## 비교 기준

| 비교 대상 | 공식 근거에서 확인한 기준 | Leftcar에 적용할 관점 |
| --- | --- | --- |
| Sunshine + Moonlight | Sunshine은 LAN/WAN별 암호화 강제 설정, 송신 비트레이트 상한, FEC 비율을 제공한다. Moonlight는 지연 단계·프레임 손실·Android 표시 페이싱의 의미를 문서화한다. [Sunshine 설정](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html), [Moonlight FAQ](https://github.com/moonlight-stream/moonlight-docs/wiki/Frequently-Asked-Questions) | UDP 전송량에 FEC/암호화 오버헤드까지 포함하고, 손실 복구와 혼잡 제어를 함께 검증한다. 이름이 같은 지연 수치도 측정 구간이 다르므로 직접 우열을 매기지 않는다. |
| RustDesk | `hbbs`/`hbbr`를 통한 자체 호스팅 연결·릴레이 구성이 있고, 클라이언트는 서버 공개키를 설정한다. 입력·클립보드·파일·오디오 권한을 각각 설정한다. [자체 호스팅](https://rustdesk.com/docs/en/self-host/), [클라이언트 설정](https://rustdesk.com/docs/en/self-host/client-configuration/), [권한 설정](https://rustdesk.com/docs/en/self-host/client-configuration/advanced-settings/) | NAT 실패 때의 연결 가능성과 각 공유 기능의 권한 경계를 별도 제품 기능으로 취급한다. 릴레이 운영·계정 정책은 패킷 크기 수정만으로 얻어지지 않는다. |
| scrcpy | ADB 기반 USB/TCP 연결이며, 영상 버퍼 기본값은 0이다. 인코딩 실패 시 낮은 해상도를 재시도하고 코덱·인코더를 선택할 수 있다. [연결](https://github.com/Genymobile/scrcpy/blob/master/doc/connection.md), [영상 정책](https://github.com/Genymobile/scrcpy/blob/master/doc/video.md) | Android 기기별 코덱 실패의 대체 경로와 최소 버퍼 정책을 참고한다. Android→컴퓨터 제품이므로 Leftcar의 컴퓨터→Android 영상이나 공개 인터넷 서버의 직접 보안 비교 대상으로 쓰지 않는다. |
| Deskflow | 영상 없는 키보드·마우스 공유이며 TLS와 사용자 지문 확인이 기본 활성화다. 클립보드 크기 상한도 설정한다. [프로젝트](https://github.com/deskflow/deskflow), [설정](https://github.com/deskflow/deskflow/blob/master/docs/Configuration.md) | 독립 화면의 입력 소유권·연결 상실 시 로컬 복귀·신뢰 확인의 기준이다. Deskflow 기능을 Android 지원이나 영상 스트리밍 성능으로 환산하지 않는다. |
| Tailscale | direct, peer relay, DERP 세 경로를 구분한다. 모두 WireGuard로 암호화되지만 지연·처리량은 다르다. 경로는 실행 중에도 바뀔 수 있다. [연결 종류](https://tailscale.com/docs/reference/connection-types), [grants](https://tailscale.com/docs/features/access-control/grants) | 100.x 주소는 오버레이 주소라는 단서이며 실제 direct 연결의 증거가 아니다. tailnet 접근 정책과 Leftcar 페어링·화면 승인을 함께 적용한다. |

## 현재 소스에서 확인한 것

여기서 **확인**은 코드를 읽고 경로를 연결했다는 뜻이다. 이 조사에서 테스트·설치·실제 스트리밍을 실행했다는 뜻이 아니다.

| 영역 | 확인한 구현 | 남은 격차·검증 범위 |
| --- | --- | --- |
| 외부 접속 | [`settings.rs`](../../apps/host-desktop/src-tauri/src/settings.rs)의 `wan_access`는 기본 OFF이며 UPnP 공개 매핑 opt-in이다. [`lib.rs`](../../apps/host-desktop/src-tauri/src/lib.rs)의 `tailscale_ip`, [`pairing.rs`](../../apps/host-desktop/src-tauri/src/pairing.rs)의 QR 별칭, [`catalog-helpers.ts`](../../apps/viewer-expo/src/catalog-helpers.ts)·[`recent-hosts.ts`](../../apps/viewer-expo/src/recent-hosts.ts)의 오버레이 경로 보존·후보 전환이 있다. | MagicDNS 자동 광고와 direct/DERP/peer-relay 실행 상태 수집은 찾지 못했다. 기존 [Tailscale 조사](2026-10-05-tailscale-remote-access.md)의 RTT만으로 DERP를 추정하는 제안은 확정 표시에 쓰면 안 된다. 혼잡·Wi-Fi 절전도 같은 증상을 만들 수 있다. |
| 인증·암호화 | [`secure-channel`](../../crates/secure-channel/src/lib.rs)에 정체키 서명·키 합의·AEAD·재생 방지가 있다. [`control.ts`](../../apps/viewer-expo/src/control.ts)는 핀을 검사하고 미등록 수동 연결은 TOFU 경로를 제공한다. [`pairing.rs`](../../apps/host-desktop/src-tauri/src/pairing.rs)는 OS 토큰 저장·철회·자격 세대를 관리한다. | 자체 프로토콜의 독립 보안 검토·fuzz 결과는 이번 조사에서 확인하지 못했다. 토큰의 정기 만료/회전 제품 정책도 확인되지 않았다. 이 사실은 취약점이 확인됐다는 뜻은 아니다. TOFU 첫 연결과 QR 핀 연결의 신뢰 차이를 안내해야 한다. |
| 입력 승인 | [`source_grants.rs`](../../apps/host-desktop/src-tauri/src/source_grants.rs)의 `input_allowed`는 **검토된 기기에 명시 거부가 없으면 기본 허용**이다. [`control.rs`](../../apps/host-desktop/src-tauri/src/control.rs)의 `standing_input_approval_auto_enables_new_sessions`도 이를 명시한다. OS 권한·자격·소스 승인 경계는 별도로 유지된다. | README·보안 문서의 “새 세션 입력 기본 OFF”는 현재 동작과 다르다. 2026-09-21 코드에 기록된 제품 결정을 조용히 뒤집지 말고, 실제 동작과 승인 화면의 문구부터 일치시켜야 한다. |
| 입력 전용 공유 | [`km.rs`](../../apps/host-desktop/src-tauri/src/km.rs)에 epoch, heartbeat, 소유권 변경 전 ReleaseAll, 배치 ACK 제한시간, 해제·로컬 복귀가 있다. Android는 Shizuku helper 권한·실제 공급자 경계가 존재한다. | Deskflow식 독립 화면 공유가 아예 없다는 평가는 틀리다. Windows backend의 unsupported와 Android/OEM·Shizuku 조건은 명확히 구분해야 한다. 입력 이벤트 시험은 실제 포인터·키 해제 수용을 대신하지 않는다. |
| macOS 영상 | [`CaptureSession+AdaptiveBitrate.swift`](../../native/macos-capture-shim/Sources/Encoder/CaptureSession+AdaptiveBitrate.swift)에 수신 손실·지연 변화·송신 막힘·다중 세션 혼잡에 따른 적응이 있다. [`Transport`](../../native/macos-capture-shim/Sources/Transport)와 [`media_datagram.rs`](../../native/android-viewer/src/media_datagram.rs)에 FEC·NACK·재조립 제한·stale 프레임 제어가 있다. | 이미 존재하는 ABR/FEC를 새 기능처럼 재구현할 필요가 없다. 외부망 손실·RTT 상승·IDR 버스트·동시 창 조건에서 정책이 실제로 회복하는지는 현재 후보로 측정해야 한다. |
| Windows 영상 | [`capture.rs`](../../apps/host-desktop/src-tauri/src/windows_backend/capture.rs)에 WGC/MFT 하드웨어 H.264·FEC 송신·키프레임 요청·송신 페이싱이 있다. 조사 시작 시 변경은 외부 경로의 1,200바이트 **평문 예산**과 송신 예산을 적용한다. AEAD 24바이트와 IP/UDP 헤더는 별도로 존재한다. | 조사 기준 [`windows_backend/mod.rs`](../../apps/host-desktop/src-tauri/src/windows_backend/mod.rs)의 UDP/TCP 수신은 BYE·IDR·입력 제어를 처리하지만 LCF1 수신 통계·지연 프로브 응답·NACK 재전송 처리를 하지 않는다. 인코더 비트레이트도 초기 생성값이다. macOS 수준 혼잡 적응·복구와 차이가 확인된다. |
| Android 표시 | [`viewer-decoder`](../../crates/viewer-decoder/src/android/decoder.rs)와 [`renderer`](../../native/android-viewer/src/renderer)에 하드웨어 디코딩·제한된 큐·vsync 기반 페이싱·latency unknown 처리·복구가 있다. | scrcpy/Moonlight보다 느리거나 빠르다는 동일 조건 증거는 없다. codec/OEM별 회복 시간·열·화면 전환·다중 Surface 수용이 필요하다. 코덱 추가보다 H.264 기준선의 반복 성공이 우선이다. |
| 클립보드·파일 | [`control.rs`](../../apps/host-desktop/src-tauri/src/control.rs)에 기본 OFF인 Host 기능 게이트, [`file_transfer.rs`](../../apps/host-desktop/src-tauri/src/file_transfer.rs)에 512MiB 파일·1MiB 청크·동시 수·임시 파일 만료·취소 제한이 있다. | 게이트는 기능 전체 단위이며 기기별/방향별 동의 모델 확장은 별도 과제다. 파일·큰 이미지가 영상과 별도이더라도 동일 FIFO 제어 TCP를 사용하므로 상태·중단 요청 지연 가능성이 있다. 이는 코드에서 추론한 위험이며 측정된 장애가 아니다. |
| 진단·배포 | [`audit.rs`](../../apps/host-desktop/src-tauri/src/audit.rs)에 제한 필드/회전 로그, [`release-manifest.mjs`](../../tools/release-manifest.mjs)에 소스·패키지 해시·서명 분류, [`release-dependencies.mjs`](../../tools/release-dependencies.mjs)에 의존성 검사 경로가 있다. [CI](../../.github/workflows/ci.yml)는 Rust/TS/Android/Windows 검사와 실기기 미검증을 구분한다. | 공개 설치판의 서명·공증·업데이트/롤백 수용은 로컬 manifest로 대체되지 않는다. 현재 CI에는 지속 fuzz·의존성 audit 실행 단계가 없다. 보안 문서의 체크리스트와 실행된 검사 결과를 구분해야 한다. |

## 개선 우선순위

| 순위 | 문제·영향 | 이번 코드 작업에서 가능한 조치 | 완료 증거 / 별도 후속 |
| --- | --- | --- | --- |
| P0 | 외부에 노출되는 제어·미디어 입력이 CPU·메모리·스레드·디스크를 오래 점유할 수 있음 | 기존 상한을 유지하며 인증 전 실패·부분 송수신·malformed 조각·FEC·연결 종료·동시 요청의 누락 경로를 회귀로 잠근다. 상한을 무작정 키워 해결하지 않는다. | 작은 결정적 악성 입력/timeout 시험, 해제 후 자원 수 확인. 공개망 공격 저항성은 별도 검증. |
| P1 | Windows 수신 피드백·혼잡 적응·재전송 격차 | 플랫폼 중립 parser/probe 응답/정직한 capability부터 공통화하고 Windows에 연결한다. macOS의 정책을 옮길 때 인코더 적용 성공과 실제 pacing rate를 함께 갱신한다. | 와이어 벡터·오래된 피드백·overflow·손실→감속→회복 시험, Windows 빌드와 MFT 실제 적용 확인. NACK ring/동적 bitrate 전체 구현은 크기 있는 후속 과제이며, 이 기능 격차 자체를 중대한 보안 결함으로 분류하지 않는다. |
| P1 | 원격 접속 실패·느려짐의 원인을 주소만 보고 판단 | LAN/overlay/public 분류와 확인된 direct/peer-relay/DERP를 다른 상태로 모델링한다. OS/CLI 정보가 없으면 unknown을 유지한다. MagicDNS는 숫자 별칭도 보존하며 추가한다. | CLI 없음·오류·stale 상태·오버레이 중단·주소 전환 fixture, 서로 다른 LAN의 실제 경로 영수증. |
| P1 | bulk 전송이 FIFO 제어를 점유 | 청크·요청 큐 상한과 backpressure를 우선 보강하고 정지·철회 지연을 측정한다. 별도 bulk 연결은 자격 수명·취소를 동일하게 묶는 설계 뒤 도입한다. | 제한 대역폭에서 파일/클립보드 전송 중 중단·철회·상태 요청 응답시간, disconnect 후 임시 파일 정리. |
| P1 | 지원·권한 설명이 실제 코드와 다름 | README/보안/지원 문서에 LAN·Tailscale·공개 WAN opt-in, 입력 기본 정책, 플랫폼 기능 표를 현재 소스와 일치시킨다. 과거 실측을 현재 후보 성능으로 쓰지 않는다. | 코드/계약/사용자 문구 비교. 실제 GUI 승인 문구는 최소 목적 화면 검토. |
| P1 | 패키지·기기·네트워크별 검증 연결이 끊김 | 기존 manifest·측정 도구를 사용해 하나의 후보 소스와 Host/APK·설치·실측을 연결한다. 새 중복 리포트 체계를 만들지 않는다. | 1080p60 시작/정지/재연결 3회 → 1440p/4K·다중 창 → 30분. Windows와 외부망은 별도 조합. |
| P2 | 사설 프로토콜의 장기 유지·배포 보안 비용 | 공통 와이어 fuzz, 잠금 파일 기반 취약점 CI, 지원 버전/보안 신고 안내를 보강한다. | 실제 실행한 scanner/fuzzer 결과와 예외 사유. 타 제품도 보안 수정이 지속되므로 “암호화 구현=안전성 완료”로 판단하지 않는다. [Sunshine 보안 수정 릴리스](https://github.com/LizardByte/Sunshine/releases/tag/v2026.914.233613) |
| P2 | 직접 연결 실패 시 제품 자체 릴레이·rendezvous 부재 | 현재는 Tailscale 운영 경로를 완성한다. 자체 STUN/ICE/TURN 또는 QUIC/WebRTC 통합은 인증·릴레이 남용 방지·운영 비용·프로토콜 호환을 포함해 설계한다. | RustDesk/Tailscale과 같은 외부 연결 완성도를 주장하려면 CGNAT·UDP 차단·망 전환·릴레이 장애 실행 결과가 필요하다. |

## 성능 비교를 실제로 닫는 방법

같은 Host/Android, 해상도/FPS/코덱, 동일 콘텐츠와 네트워크에서 Leftcar와 Sunshine/Moonlight를 비교한다. 정적 화면은 낮은 FPS가 정상일 수 있으므로 애니메이션·스크롤·입력 반응을 따로 측정한다. 앱이 보고하는 RTT, 디코드, 프레임 큐와 광학 입력→화면 지연을 섞지 않는다. Moonlight도 내부 통계만으로 다른 클라이언트와 지연을 직접 비교할 수 없다고 설명한다. [측정 의미](https://github.com/moonlight-stream/moonlight-docs/wiki/Frequently-Asked-Questions#what-do-the-numbers-mean-in-the-on-screen-connection-statistics)

외부망은 최소한 직접 오버레이, 릴레이 오버레이, 대역폭 감소, RTT 증가, 손실/재정렬, 경로 변경을 구분한다. 관찰값은 p50/p95/p99 지연, 렌더 FPS·누락, 복구 시간, 송신/수신량, FEC/재전송 비용, 큐 나이, CPU·메모리·열 상태다. 스크린 캡처를 반복하기 전에 기존 로그·카운터를 사용하고 광학 측정은 해소할 주장에 필요한 최소 범위로 한다.

암호화·승인·철회 실패를 성능 향상을 위해 완화하지 않는다. WAN/릴레이가 느릴 때 비트레이트·해상도·FPS·버퍼 예산을 조정하며, 확인할 수 없는 연결 경로나 측정값은 unknown으로 둔다.

## 이번 작업의 제한된 경로 진단

`bun tools/network-diagnostics.mjs --peer <tailnet-IP-or-full-MagicDNS-name>`은 사용자가 지정한 피어 하나의 로컬 Tailscale 상태만 요약한다. 명령 시간은 5초, 읽는 출력은 4MiB로 제한하며 원본 JSON·피어 목록·이름·IP·키·로그인 URL·stderr는 출력하지 않는다. 설정 변경이나 ping은 하지 않는다.

`recentRoute`는 공식 CLI와 같은 `CurAddr` → `PeerRelay` → `Relay` 우선순위를 따르되 Running/Online/Active가 확인될 때만 표시한다. `Active`는 최근 통신을 뜻하므로 결과의 `streamReachability`는 항상 `unverified`다. 저장된 Relay 값이나 높은 RTT만으로 현재 DERP 경로라고 단정하지 않는다. CLI 없음·미확인/유휴 상태·스키마 차이는 unknown 또는 명시적 오류로 남긴다. [공식 상태 스키마](https://github.com/tailscale/tailscale/blob/main/ipn/ipnstate/ipnstate.go), [공식 CLI 경로 표시](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/status.go)

이 도구는 진단 자료 수집을 보완하며 앱 UI의 연결 경로 표시나 자동 품질 조정, 실제 외부망 수용 완료를 뜻하지 않는다.

## 비교 후 구현한 내용과 남은 격차

아래는 2026-10-13 작업 트리의 소스와 해당 회귀 시험 기준이다. 위의 개선 전 표에서 찾은 모든 격차가 해소됐다는 의미는 아니다. 전체 통합 검사·패키지 검증 결과는 [작업 계획](../plans/2026-10-13-performance-security.md)의 최종 기록과 함께 확인한다.

| 영역 | 이번에 구현한 변화 | 여전히 필요한 증거·후속 |
| --- | --- | --- |
| 제어 연결·키 합의 | Host는 원격 IP별 8개 연결과 인증 전 총 30초 수명을 적용한다. Rust 키 합의가 비기여 X25519 키를 거부하며, Viewer도 길이·키 검증과 비밀 정리를 보강했다. | 분산 발신자의 공개망 부하 시험·독립 암호 검토·지속 fuzz는 별도다. 로컬 프록시는 원격 IP별 제한 대신 기존 전역 64개 제한을 적용한다. |
| UPnP 수명 | 활성화·해제 의도를 작업 생성 시 확정하고, 늦게 시작한 이전 작업이 새 설정을 뒤집지 못하게 했다. 기존 매핑의 소유권을 확인하고 부분 실패·응답 유실·취소 때 확인된 소유 매핑만 정리한다. 외부 IP 확인 실패·비공개 주소에서는 광고를 철회한다. | 실제 공유기·CGNAT·망 변경은 미검증이다. 응답하지 않는 공유기의 삭제 성공을 보장하지 않으며 남은 매핑은 1시간 lease 만료에 의존할 수 있다. |
| FIFO·파일 안전성 | Viewer는 32개/16MiB 요청 예산과 암호화 후 와이어 길이를 쓰기 전에 검사한다. 새 요청 거부는 기존 FIFO를 보존한다. 파일 송신은 일반 파일만 받고 Begin 때 연 파일 핸들을 유지한다. | 별도 bulk 연결·정지/철회 지연 측정은 남았다. 파일 핸들 고정은 경로 교체 방지이며 동일 파일 내용의 변경을 막는 스냅샷·무결성 해시는 아니다. |
| 영상 자원·복구 비용 | FEC는 정상 수신 시 행렬 복원을 생략하고 손실 shard만 복원한다. Android는 작은 MTU 조각을 기존 버퍼에서 합치고 잘못된 조각을 할당 전에 거부한다. 표시 큐의 나이·용량 중복 제거와 코덱 이름 할당 해제, macOS 혼잡 기억의 만료·128개 상한을 보강했다. | 할당·알고리즘 경로 개선은 코드로 확인되지만 실제 FPS·지연·열 개선 수치는 현재 패키지로 다시 측정해야 한다. |
| Windows 피드백 | 공통 [`media_feedback.rs`](../../apps/host-desktop/src-tauri/src/media_feedback.rs)가 LCF1 통계와 LCP1→LCP2 응답을 처리한다. 인증된 제어 응답은 Viewer의 별도 송신 포트로 보내며 미디어 목적지는 유지한다. | 이 변경만으로 NACK 재전송 ring이나 MFT 동적 비트레이트 제어가 생기지는 않는다. Windows 실행·NAT 통과·인코더 적용과 macOS 수준 적응의 격차가 남는다. |
| 경로 진단·검증 | 지정 피어 하나의 제한된 Tailscale 상태 진단을 추가했다. README/지원/보안 문서의 외부 접속·입력 정책을 맞추고, CI에 원격 MTU·혼잡·페이싱 정책 회귀를 연결했다. | 실제 경로 접근성, 경로 UI, 최신 후보의 배포 서명·공증·업데이트/롤백·기기별 장시간 수용은 별도다. CI 설정 추가는 원격 CI 실행 성공의 증거가 아니다. |

독립 재실행: Viewer 제어·카탈로그·키 합의와 경로 진단 4개 파일의 **103개 테스트 통과**. 담당 구현자의 최종 범위별 실행 기록: Rust secure-channel **16개 통과**(fixture 출력용 2개 ignored), UPnP **21개 통과**, 파일 전송 **20개 통과**. secure-channel은 순방향 UDP마다 발생하던 replay-window 임시 할당도 제거했다. UPnP 회귀는 가짜 로컬 공유기의 소유권·부분 실패·적용 후 응답 유실·취소·작업 순서 역전을 검증한다. 전체 검사·Clippy·의존성 및 구조 경고는 [최종 로컬 검증](../plans/2026-10-13-performance-security.md#최종-로컬-검증)을 따른다. 실제 공유기·외부망·Windows 기기 수용 결과로 확대 해석하지 않는다.
