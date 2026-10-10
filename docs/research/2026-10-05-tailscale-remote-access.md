# Tailscale 원격 접속 현황 조사 및 실행 구현 계획

- 작성일: 2026-10-05
- 범위: 호스트가 tailnet에 있을 때 뷰어가 LAN 없이 MagicDNS/tailnet IP로 바로 접속하는 시나리오
- 조사 대상: `DESIGN.md`, `apps/host-desktop/**`, `apps/viewer-expo/src/**`, `artifacts/device/**`, `crates/**`, `docs/**`

## 1. 현재 지원 범위 (요약)

Tailscale 직접 접속은 이미 **1등 시민으로 부분 구현**되어 있다. 빈틈은 "연결 자체"가 아니라 "이름(MagicDNS), 검증, 문서" 쪽에 있다.

### 호스트 측 (`apps/host-desktop/src-tauri`)

| 기능 | 위치 | 내용 |
| --- | --- | --- |
| 테일넷 자기 주소 감지 | `apps/host-desktop/src-tauri/src/lib.rs:1486` (`tailscale_ip`) | UDP connect 트릭(`100.100.100.100:80`, 패킷 미전송)으로 100.64/10 인터페이스 주소를 읽는다. 테일넷이 내려가면 None. `tailscale` CLI 의존 없음. |
| 카탈로그 광고 | `apps/host-desktop/src-tauri/src/control.rs:3135` | `getCatalog` 응답에 `tailscaleHost`를 포함. |
| 계약 필드 | `crates/control-contract/src/host.rs:240` (`tailscale_host: Option<String>`) | "테일넷 인터페이스가 살아 있을 때만 온다. 뷰어는 별칭으로 저장." 구버전 호스트는 생략. |
| QR 별칭 | `apps/host-desktop/src-tauri/src/pairing.rs:302-373` | 페어링 QR에 `ts` 필드로 테일넷 별칭 실음 (`begin_pairing_with_tailnet`). |
| 컨트롤 리슨 | `apps/host-desktop/src-tauri/src/lib.rs:727` | `0.0.0.0` 바인딩 — 테일넷 인터페이스 도달 가능. |
| 접근 정책 | `apps/host-desktop/src-tauri/src/settings.rs:40` | `wan_access` 토글은 UPnP/공인 노출 전용. **Tailscale 오버레이는 토글 없이 항상 허용**되며, 이것이 의도된 정식 원격 경로다. |
| tailnet 피어 인증 | `apps/host-desktop/src-tauri/src/control.rs:3048, 3229` | tailnet에서 온 연결도 페어링 서명·피어 라우트 유지 정책을 적용. |

### 뷰어 측 (`apps/viewer-expo/src`)

| 기능 | 위치 | 내용 |
| --- | --- | --- |
| 신뢰 판정 | `apps/viewer-expo/src/pairing.ts:141-161` (`isTrustedHost`) | `.ts.net` 이름과 100.64/10(CGNAT)을 신뢰 경로로 수용. |
| 테일넷 판정 | `apps/viewer-expo/src/catalog-helpers.ts:64` (`isTailscale`) | `.ts.net` 접미사 또는 100.64/10 숫자 판정. |
| 미디어 라우트 선택 | `apps/viewer-expo/src/catalog-helpers.ts:82-101` (`catalogMediaHost`) | 선택된 주소가 테일넷이면 **컨트롤과 미디어 모두 테일넷 라우트를 유지** (LAN 광고 주소로 떨어지지 않음). |
| 별칭 병합 | `apps/viewer-expo/src/recent-hosts.ts:219` (`mergeAdvertisedRoutes`) | 카탈로그가 광고한 `tailscaleHost`·`mediaHost`를 최근 호스트 엔트리 별칭으로 병합 — 집에서 한 번 연결하면 어느 네트워크에서든 후보 확보. `apps/viewer-expo/src/use-catalog-model.ts:238-252`가 카탈로그 화면 백스톱으로 호출. |
| 접속 후보 순위 | `apps/viewer-expo/src/recent-hosts.ts:248` (`resolveConnectCandidates`) | 같은 /24 LAN → **테일넷(어디서든 도달)** → 낯선 사설망/mDNS → 공인 순. `RecentHostQuickConnect.tsx`가 `connectHostWithFallback`으로 순차 페일오버. |
| 외부 경로 완화 | `apps/viewer-expo/src/streaming-policy.ts:35` + `apps/viewer-expo/src/use-catalog-model.ts:234,754,905,1058` | 테일넷/공인 경로면 시작 스트림 타깃을 1080p 숏사이드로 캡. 상향은 적응 정책 소관. |
| 소켓 피어 식별 | `apps/viewer-expo/src/control.ts:264` | MagicDNS 이름 해석을 포함한 연결 피어 주소를 `remoteAddress`로 노출. |
| 미디어 MTU | `crates/media-model/src/fragment.rs:11` (`DEFAULT_MTU = 1200`) | 프래그먼트가 1,200바이트 이하 — tailnet 인터페이스 MTU 1,280(WireGuard 오버헤드 포함)에 맞는다. 별도 작업 불필요. |

결론: **tailnet IP(100.x)로 컨트롤+미디어 직접 접속, 별칭 저장, 후보 페일오버, 대역폭 완화까지 이미 동작한다.**

## 2. 빈틈 (Gaps)

1. **G1 — MagicDNS 이름 미광고 (1순위 시나리오의 핵심 빈틈).** 호스트는 숫자 100.x IP만 광고한다(`tailscale_ip`). 뷰어는 `.ts.net` 이름을 수용하지만(`isTailscale`, `isTrustedHost`), 어느 계층도 MagicDNS 이름(`leftcar-host.tail1234.ts.net`)을 생성·전달하지 않는다. 사용자가 이름을 수동 입력하는 경우에만 유효. 숫자 IP는 tailnet 재구성 시 바뀔 수 있어 이름이 더 안정적인 식별자다.
2. **G2 — tailnet 시나리오 end-to-end 검증 부재.** `docs/`에 Tailscale 관련 검증 문서가 없다(wifi-km·USB 계열만 존재). "LAN 없이 테일넷만으로 페어링→카탈로그→스트림→입력" 전 경로의 실측 기록이 없어 회귀 시 눈치챌 방법이 없다.
3. **G3 — 원격 최초 페어링 경로.** QR 페어링은 같은 화면 전제다. 뷰어가 이미 원격에 있을 때는 (a) 이전 별칭 병합이 있거나 (b) 수동 host+PIN 입력뿐인데, (b)에서 `.ts.net` 이름 입력이 실제로 끝까지 동작하는지 검증 기록이 없다.
4. **G4 — 문서 공백.** `DESIGN.md`는 Tailscale을 벤치마크 참조로만 언급하고, `docs/03-system-architecture.md`·`docs/07-security-privacy.md`에는 오버레이 원격 경로의 위상과 보안 모델(tailnet 피어여도 페어링 서명 필수, `wan_access`와 무관)이 기술되지 않았다.
5. **G5 — DERP 릴레이 폴백 품질.** 직접 경로 실패 시 Tailscale이 DERP 릴레이로 폴백하며 대역폭·지연이 급변한다. `externalRoute` 시작 캡은 있지만 릴레이 폴백을 감지하거나 사용자에게 안내하는 장치는 없다(선택 개선).
6. **G6 — 뷰어 간 패리티 미확인.** `apps/viewer-android`가 동일한 테일넷 경로를 지원하는지 조사 범위(`viewer-expo`만) 밖이라 미확인. 리스크로 기록한다.

빈틈 수: **6** (G1·G2가 1순위).

## 3. 계획된 작업 (Plan)

| # | 작업 | 내용 | 주요 파일 | 우선순위 |
| --- | --- | --- | --- | --- |
| P1 | tailnet end-to-end 실측 검증 | 호스트·뷰어를 서로 다른 LAN에 두고 테일넷만으로 페어링(수동 host+PIN 포함)→카탈로그→스트림→입력 실측. 결과를 `docs/research/tailscale-direct-validation.md`로 기록. 계측은 기존 receiver RTT/비트레이트 필드 재사용. | docs/research/ | **P1 (먼저)** |
| P2 | MagicDNS 이름 광고 | 호스트가 자기 MagicDNS 이름을 얻어(`tailscale status --json --peers=false`의 `Self.DNSName`, CLI 부재 시 기존 `tailscale_ip`로 조용히 폴백) `CatalogView`에 `tailscaleMagicdns`(Option)로 추가하고, QR `ts` 필드는 이름을 우선 사용. 뷰어는 기존 `isTailscale`/`isTrustedHost`가 이미 수용하므로 라우트 선택·별칭 병합은 무수정. | `crates/control-contract/src/host.rs`, `apps/host-desktop/src-tauri/src/{lib,control,pairing}.rs` | **P2** |
| P3 | 문서 보완 | `docs/03-system-architecture.md` 원격 접속 절과 `docs/07-security-privacy.md`에 오버레이 경로 보안 모델(tailnet 피어도 페어링 서명 필수, `wan_access` 토글과 독립) 기술. | docs/03, docs/07 | P3 |
| P4 | DERP 폴백 게이트 (선택) | receiver RTT 급증 + 스루풋 붕괴 조합을 "릴레이 경로"로 추정해 UI 안내. 신규 계약 필드 없이 기존 상태 텔레메트리만 사용. | `apps/viewer-expo/src/use-catalog-model.ts` 주변 | P4 (선택) |
| P5 | viewer-android 패리티 확인 | G6 해소. 동일 `catalog-helpers`·`recent-hosts` 로직 공유 여부 확인 후 빈틈이면 별도 작업 티켓으로 분리. | apps/viewer-android | P5 |

계획된 작업 수: **5** (P4는 선택).

## 4. 가장 위험한 변경 지점

**P2의 계약 확장 — `crates/control-contract/src/host.rs`의 `CatalogView`에 `tailscale_magicdns` 필드 추가와 QR 페이로드 변경이 가장 위험하다.**

- `CatalogView`는 호스트↔뷰어 버전 호환의 경계다. `Option + skip_serializing_if` 패턴(`tailscale_host`가 이미 그렇듯)을 그대로 따르면 구버전 상호운용은 안전하지만, 직렬화 스냅샷·계약 테스트(`crates/control-contract/tests/contract.rs`) 갱신이 강제된다.
- QR `ts` 필드에 `.ts.net` 이름이 실리면 현재 뷰어 코드는 `isTrustedHost`가 수용하지만, **필드를 숫자 IP로 가정했던 구버전 뷰어 빌드**가 있는지 확인이 필요하다. 불확실하면 이름은 QR이 아닌 `CatalogView`에만 넣는 완화안(대안 B)으로 단계 진행한다.
- 대안 A(위 계획): 이름+IP 모두 광고, QR은 이름 우선. 대안 B: QR은 현행 숫자 IP 유지, 이름은 카탈로그에서만 병합. A가 목적(안정 식별자)에 부합하나 구버전 QR 소비자 검증을 전제로 한다.

차위 위험: P1을 P2보다 먼저 실행하는 이유 — MagicDNS 광고는 기존 100.x 경로가 실제로 끝까지 동작함이 확인된 후에야 의미가 있고, 검증에서 발견되는 빈틈이 P2 설계(예: 이름 해석 실패 시 폴백)를 바꿀 수 있다.

## 5. 미해결 리스크

- `apps/viewer-android`의 테일넷 지원 여부 미확인 (G6, P5에서 해소).
- 구버전 뷰어의 QR `ts` 필드 숫자 가정 여부 미확인 (P2 착수 전 QR 소비자 이력 확인 필요).
- DERP 릴레이 폴백 시의 실측 품질 데이터 없음 (P4 근거 자료, P1 실측에 함께 기록 권장).
