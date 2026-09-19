# ADB 실기 검증 및 재연결 개선 — 2026-09-16

## 현재 결론

**전체 성능 판정은 NOT_ACCEPTED다.** 기존 기기의 정상 재시작 후 승인 유지, 1440p 영상 창의 73초 백그라운드 복귀, 메인 목록 창을 닫아도 영상 유지, 마지막 영상 창 종료 시 서비스 정리는 설치본에서 검증했다. 최신 고동작 측정 S6은 57.36fps지만 지연 p95 150ms·복구 8회로 실패했다. 한·영 물리 입력과 4K 복귀는 미검증이다.

성능 측정 중 부하 페이지의 실행과 실제 캡처 화면의 일치가 별개의 조건임을 발견했다. 아래 S2~S4는 연결·출력 진단 기록으로만 사용하고, 통제된 고동작 성능 비교에서는 제외한다.

사용자는 ADB 버튼 누르기, 앱 전환, Lenovo Tailscale의 일시 해제와 복원을 허용했다. VPN은 비교 후 다시 켰다.

## 완료한 실기 측정

| 조건 | 측정 | 결과 |
| --- | --- | --- |
| S2: 1440p60 / Tailscale 직접 연결 / UDP auto | 181.28초 | 진단 기준 FAIL, 캡처된 부하 화면 미검증 |
| S2: 같은 조건 / custom16 | 다른 앱의 전면 전환으로 활성 세션이 2개 샘플에만 존재 | EXCLUDED |
| S3: 1440p60 / Tailscale / custom16 / MTU 수정 | 181.23초 | 진단 기준 FAIL, 캡처된 부하 화면 미검증 |
| S4: 1440p60 / 일반 Wi-Fi / custom16 | 181.49초 | 진단 기준 FAIL, 캡처된 부하 화면 미검증 |
| S5: 1440p60 / Tailscale / auto / 실제 Chrome 전체 화면 부하 | 181.52초 | 고동작 전달 확인, 성능 FAIL |
| S6: S5 조건 / 연결 유지 서비스 수정 APK | 181.31초 | 고동작 전달 확인, 성능 FAIL |

auto 측정의 소프트웨어 출력은 57.98fps였다. 57fps 이상인 샘플이 143/181개로 90% 기준에 미달했다. 캡처→Surface release 지연 스냅샷 p95는 124ms(기준 50ms), RTT p95는 43ms(기준 20ms), 복구 키프레임은 5회 추가됐다. 프레임 gap·송신 실패·비트레이트 하한 압박 증가는 0이었다. 원본 부하는 전체 화면 3840×2160, 약 59.97fps로 정상 구동했다.

이 시험 중 태블릿의 다른 영상 재생과 Mac의 다른 작업 부하가 존재했다. 무부하 환경의 순수 전송 성능으로 해석하지 않는다. 구간별 JSON 원본을 보존한다.

S3의 소프트웨어 출력은 58.05fps, 캡처→Surface release 스냅샷 p95 129ms, RTT p95 52ms, 새 gap 1회·복구 1회였다. S4는 RTT p95 45ms, 새 gap 27회·복구 98회였고 출력 기록 연속성이 깨져 전체 FPS와 출력 지연 판정을 확정할 수 없었다. 정상 표본의 지연 스냅샷 p95 118ms만으로 통과를 주장하지 않는다. 두 경로에서 ABR이 고른 비트레이트도 달랐으므로 경로 하나만 바꾼 실험으로 해석하지 않는다.

### 캡처 화면 일치 검증 보완

IAB의 `visible`, `focused`, `fullscreen`, 60fps 기록은 브라우저 내부 상태다. 실제 Host Display 0이 그 페이지를 캡처한다는 증거가 아니었다. 이후 태블릿 화면에서 다른 데스크톱 장면이 관찰됐다. 이 관찰로 과거 전체 구간의 화면을 단정할 수도 없으므로, 기존 결과를 소급해서 고동작 합격 또는 경로별 우열의 근거로 쓰지 않는다.

수정된 검증은 네이티브 macOS Chrome 창에 시험 장면을 전체 화면으로 표시하고, 태블릿에서 같은 질감·도형·시간 표시가 수신되는지 직접 확인한다. IAB의 부하 페이지는 종료해 중복 cadence 기록을 제거했다. 수치는 계속 JSON으로 수집하고 화면 확인은 부하의 실제 전달 여부에 한정한다.

### 실제 고동작이 전달된 S5

S5는 시작 전과 종료 후 태블릿에서 시험 장면을 확인했고, 구간 내 Chrome 부하 cadence 182개·태블릿 전면 상태 59개·출력 347개 표본이 연속됐다. 2560×1440@60, HEVC RTVC 인코더, UDP 자동(4/FEC 2), 적응형 전송 조건이다. 장면은 3840×2160 전체 화면에서 59.97fps로 움직였다.

- 소프트웨어 출력: **57.29fps**.
- 캡처→Surface release 지연 스냅샷: 중앙값 **50ms**, p95 **143ms**. 기준 p95 50ms 이하 실패.
- RTT 스냅샷: 중앙값 **25ms**, p95 **47ms**. 기준 p95 20ms 이하 실패.
- 신규 frame gap **1**, 미완성 access unit **10**, 복구 키프레임 **9**. 송신 실패와 비트레이트 하한 압박 증가는 0.

이 결과는 높은 평균 FPS가 낮은 지연과 안정성을 보장하지 않음을 보여 준다. 직접 P2P/UDP임에도 지연 꼬리와 영상 복구가 남았다. Host 캡처·인코더는 대체로 60fps였지만, 전송 간격 p95 스냅샷 21.31ms와 RTT 변동이 관찰됐다. 이 수치들은 병목 후보이며 단독 원인을 확정하는 실험은 아니다. 1080p로 낮추거나 합격 기준을 완화하지 않았다.

### 최신 설치본 S6 재검증

연결 유지 서비스를 포함한 최신 APK에서 같은 1440p60·HEVC RTVC·자동 전송 조건으로 181.31초를 다시 측정했다. 실제 Chrome 장면의 태블릿 전달을 시작 전·종료 후 확인했고 소스 182개·전면 상태 59개·출력 346개 표본이 정상 연결됐다.

| 항목 | S6 결과 | 기준 |
| --- | ---: | ---: |
| 소프트웨어 출력 평균 | 57.36fps | 60fps 목표, 57fps 이상 표본 90% 이상 |
| 캡처→Surface release 지연 중앙값 / p95 | 56 / **150ms** | p95 ≤ 50ms |
| RTT 중앙값 / p95 | 25 / **47ms** | p95 ≤ 20ms |
| 새 gap / 미완성 AU / 복구 키프레임 | **1 / 8 / 8** | gap·복구 0 |
| 송신 실패 / 비트레이트 하한 압박 증가 | 0 / 0 | 0 / 0 |

서비스 수정은 앱 이탈 시 연결 차단을 해결했지만 전면 영상의 저지연 성능을 합격 수준으로 바꾸지는 않았다. 스냅샷 p95는 개별 프레임 전체의 p95와 다르며 실제 패널 발광 시점을 측정한 수치도 아니다.

## 다른 앱에서 돌아올 때 연결 종료

S5 설치본에서 홈으로 나갔다가 약 30초 뒤 실제 영상 창으로 돌아오면 검은 화면·0fps가 재현됐다. Host는 `viewer connection lost (feedback timeout)`으로 세션을 종료했다. Android 수신 worker는 백그라운드에서도 heartbeat를 보내려 했지만, 약 5초 후부터 매초 `Operation not permitted (os error 1)`로 전송이 거절됐다. 단순히 수신 thread가 정지했다고 판단하지 않는다.

이에 영상 창이 살아 있는 동안 `connectedDevice` foreground service로 네트워크 연결을 유지하도록 수정했다. 메인 목록 창과 소유권을 분리하고, 여러 영상 창 중 마지막 창이 종료될 때 서비스를 멈춘다. 실제 수신·표시 worker 구조와 인증은 그대로 사용한다. 새 연결 권한을 부여하는 기능은 아니다. Android가 프로세스를 종료한 뒤 서비스를 무조건 재생성하지 않도록 `START_NOT_STICKY`를 사용한다.

Android 생명주기 회귀 검사는 숨김, 두 창 중 하나 닫기, 반복 전면 복귀 후 마지막 창 닫기를 다룬다. 알림 리소스까지 JVM에서 로딩하려던 시도는 기존 `manifest=NONE` 테스트 환경과 충돌해 제거했으며, 알림·foreground 실행 여부는 실기에서 별도로 검증한다.

**수정본 실기 복귀 PASS:** APK SHA `8432ffeb1056c6df042a47fe50c19e57295ec0b6e666c191259c7ac746c45b5d`를 설치하고 readback 일치를 확인했다. 홈 및 최근 앱 화면에 약 73.82초 머문 뒤 동일 영상 창으로 돌아왔다. Host 세션 6은 120개 상태 표본에서 계속 running이었고, 백그라운드 23개 시스템 표본 모두 foreground service가 실행 중이었다. 전송 EPERM은 0회, 같은 native transport를 replacement Surface에 다시 붙인 로그와 움직이는 장면 복귀를 확인했다. 이 결과는 1440p 단일 창 복귀 검증이며 4K split, 프로세스 사망 후 복구, 장시간 배터리 영향은 포함하지 않는다. 화면이 가려진 동안에도 네트워크 연결과 영상 수신은 유지한다.

Android 전체 136개 검사·typecheck·React Doctor 100/100·APK 빌드가 통과했다. 최종 빌드 로그는 `connection-service-build-final.log`다.

추가로 Android 최근 앱에서 **메인 목록 창만 제거한 후에도** 같은 영상 창과 Host 세션 6이 유지되고 영상이 출력되는 것을 확인했다. 마지막 영상 창의 닫기 확인을 선택하자 Activity가 종료됐고 약 21ms 뒤 서비스 종료 로그가 나왔다. 최종 시스템 조회에 서비스가 없고 Host 세션 목록도 비었다. 약 21ms는 로그 간격이며 제품 보장값은 아니다. 시험 부하는 종료했고 Tailscale은 켜진 상태, 전송 설정은 자동으로 남겼다.

## 키보드 입력 확인 범위

ADB로 보낸 영문 키는 Mac의 로컬 입력 검증 페이지에 도달했다. Caps Lock을 섞어 `a`, `g`, `k`, `s` 등을 입력했지만 이 실험에서는 한글 조합이 확인되지 않았다. Android의 현재 Gboard subtype은 빈 locale과 `en-Latn-US` 물리 키보드 힌트를 보고했다. 사용자가 설정한 물리 Caps Lock과 ADB keycode 주입은 같지 않으므로, 물리 한·영 전환은 미검증 상태로 남긴다. 사용자 키 매핑이나 전역 입력 설정을 바꾸지 않았다.

### VPN과 기존 LAN 주소

Lenovo에서 VPN이 켜진 상태의 `192.168.0.134` 경로는 `tun1`이었고 연결이 타임아웃됐다. Tailscale을 끄자 `wlan0`으로 바뀌고 ping 8.24/8.78ms, 저장된 기존 자격으로 목록·영상 연결에 성공했다. Mac에서 LAN 목적지는 `en0` MTU 1500, tailnet 목적지는 `utun3` MTU 1280이었다. 비교 후 Tailscale의 Connected 상태와 tailnet ping 복원을 확인했다.

현재 환경에서는 VPN을 유지할 때 Tailscale 주소를 사용하는 것이 작동한다. LAN 실패를 앱 승인 문제로 해석하지 않으며, 사용자 VPN의 라우팅 설정을 영구 변경하지 않았다.

## P2P / UDP / TCP 확인

- 현재 Mac–Lenovo Tailscale 경로는 직접 P2P다. `tailscale ping`이 LAN peer endpoint를 보고했다. 이는 같은 LAN의 직접 연결 증거이며 외부 WAN 성능 증거는 아니다.
- 영상은 UDP, 입력은 별도 UDP worker로 처리된다. 중요 입력은 앱 계층의 ACK와 재전송을 사용한다.
- TCP는 페어링, 화면 목록, 스트림 시작/종료 등 연결 관리에 사용한다.
- P2P는 경로, UDP/TCP는 전송 방식이다. 영상의 UDP 사용과 연결 관리의 TCP 사용은 함께 가능하다.

## Tailscale 패킷 크기 수정

Mac의 실제 `utun3` MTU는 1280이었다. 기존 영상 plaintext 1400바이트에 AEAD 24바이트와 IPv4/UDP 28바이트가 붙으면 1452바이트가 된다. 직접 P2P여도 이 터널 제한은 적용된다.

공유 패킷 조립 경로에서 목적지가 `100.64.0.0/10`이면 plaintext를 1200바이트(암호화 IP 패킷 1252바이트)로 제한했다. 일반 LAN은 1400바이트, USB는 기존 조각 폭을 유지한다. 비트레이트와 전송 간격 설정은 바꾸지 않았다.

- RED: 실제 production packetizer와 암호화에서 `1452 > 1280` 실패 재현.
- GREEN: 6개 주소 경계 × 5개 payload 크기 × keyframe 여부 = 60개 검사 통과. FEC, 암호화 복원, 영상 재조립 포함.
- 기존 인코더/전송 정책 검사 통과.
- 기존 split pipeline 검사는 sandbox 실행이 중단되어 시스템 접근을 허용한 동일 바이너리로 재실행했고 통과했다.
- 수정 shim 설치 시 기존 Host 실행 코드는 유지하고 서명만 갱신했다. export 15개 보존, 지정 서명 요구사항 일치, 이전 앱 백업 보존.

이 수정은 패킷 크기의 정확성 수정이다. S3에서 실기 지연 개선은 확인하지 못했고, 캡처 부하 검증 부족으로 이전 결과와의 통제 비교도 성립하지 않는다. packetization 회귀 검사 통과를 성능 합격으로 취급하지 않는다. 공격적인 custom16은 기본값으로 채택하지 않았고 Lenovo의 전송 설정을 자동으로 복원했다.

## 선택별 장점과 판단

| 선택 | 관찰한 장점 | 한계와 현재 판단 |
| --- | --- | --- |
| Tailscale 직접 P2P + 영상 UDP | 외부 주소로 기존 인증·영상 경로 사용. TCP의 순차 재전송 대기를 영상에 강제하지 않음 | 같은 LAN에서 직접 연결을 검증했으며 WAN 성능은 미검증. 터널 MTU와 Wi-Fi 변동은 남음 |
| 자동 전송 | 기존 적응형 속도/FEC 정책 사용 | 실제 고동작에서도 지연 기준 미달. 검증되지 않은 공격적 옵션 대신 현재 설정으로 복원 |
| custom16 | Host 대기 재생 시험에서 프레임 이월 포함 p95 중앙값 22.69→11.40ms | 실제 고동작 우위 증거 없음. 패킷을 몰아 보내 수신 밀림·손실이 늘 수 있어 기본 채택 보류 |
| Tailscale용 1200바이트 조각 | 암호화 후 IPv4 패킷 1252바이트로 MTU 1280 내에 들어감 | 같은 영상을 더 많은 패킷으로 나누므로 호출 비용이 늘 수 있음. 지연 향상은 미입증 |
| 창이 소유하는 연결 유지 서비스 | 73초 앱 이탈 후 같은 연결·영상 창 복귀 확인 | 숨겨진 동안에도 수신·연결 비용 발생. 마지막 영상 창 종료 시 정리 |

별도 Host 인코더 시험은 **H.264 합성 프레임→callback 약 15초**이며, 실제 S5 영상의 **HEVC** 경로와 직접 비교하지 않는다. 1440p RTVC는 유효 59.49fps·callback p95 11.61ms·900개 중 8개 drop, AVE는 59.90fps·38.49ms·drop 0이었다. 4K 두 타일에서는 RTVC 유효 44.96fps·타일당 drop 107개, AVE 59.89fps·drop 0이었지만 callback p95는 약 40ms였다. 이 시험에서는 RTVC의 짧은 처리 지연과 AVE의 처리량 유지가 각각 장점이었다. 캡처·네트워크·태블릿 디코더를 포함하지 않아 인코더 기본값 변경이나 4K 실기 합격 근거로 사용하지 않는다.

## 승인 반복과 재연결

수정 Host를 적용하려고 SIGTERM으로 종료한 뒤, 기존 화면 권한이 재검토 상태로 바뀌었다. 기존 정책은 정상 종료 후의 승인은 유지하지만 비정상 종료 후에는 승인 취소의 저장 성공 여부를 확신할 수 없어 재검토를 요구한다. SIGTERM이 Tauri의 정상 종료 경로를 거치지 않은 것이 이번 재승인 원인이다.

개선 방향은 인증된 기존 기기의 재접속과 정상 업데이트에서 기존 화면·입력 승인을 재사용하는 것이다. IP 주소로 새로운 신뢰를 부여하지 않는다. 신규 기기, 자격 변경, 명시적 취소, 승인 저장 실패는 별개로 다룬다.

- SIGTERM을 Tauri Exit 경로로 연결했다. 설치한 새 Host에서 실제 SIGTERM 종료 후 clean journal을 확인했고, 재실행 후 기존 2개 기기의 화면·입력 승인 기록이 동일하게 유지됐다. 이 검사에서 새 승인은 하지 않았다.
- 기존 SourceGrantEditor가 기기 목록에 연결되지 않아 권한을 복구할 수 없던 UI를 연결했다.
- 임시 연결 코드 생성 버튼과 기기 목록 창 열기는 별도 동작이다. 코드 생성 없이 목록을 여는 동작임을 소스로 확인했다. 자동 검토가 처음에는 버튼 이름을 근거로 차단했으나, 구현 근거 확인 후 목록 조회는 허용됐다.
- Host 전체 Rust 회귀 검사 229개, 관련 TypeScript 28개, 전체 typecheck, React Doctor 100/100을 통과했다.
- 실제 강제 종료나 승인 저장 불확실성의 재검토 정책은 유지한다. 자동 복구를 위해 권한 파일을 직접 고치거나 인증을 생략하지 않는다.

## 저장 공간

사용자 요청에 따라 재생성 가능한 빌드 캐시만 정리했다. Host debug와 사용하지 않는 Windows target 캐시를 정리한 시점에 여유 공간은 약 1.7GB에서 12.1GB로 늘었다. 소스, Git, 앱 데이터, 설치본 백업, APK 및 성능 근거는 보존했다. 이후 빌드에 따라 여유 공간은 달라진다.

## 근거와 경계

JSON 및 로그: `artifacts/performance-acceptance-2026-09-16/`.

- `split-resume-installed-receipt.json`: Lenovo APK SHA `b4dfb87613ce6dff547617be1d3501985f6074c15b7e231a1c115a9884ec0c4e`, 설치 후 readback 일치.
- `s2-tailnet-1440-auto-metrics.json`, `s3-tailnet-1440-custom16-mtu1200-metrics.json`, `s4-lan-1440-custom16-metrics.json`: 진단 측정. 아래 workload audit의 제한이 우선한다.
- `workload-binding-audit.json`: 실제 캡처 부하 일치 검증의 누락과 기존 측정의 사용 제한.
- `s5-tailnet-1440-auto-visible-metrics.json`, `s5-tailnet-1440-auto-visible-workload-binding.json`: 실제 고동작 전달을 확인한 측정과 실패 기준.
- `s6-tailnet-1440-auto-fgs-metrics.json`, `s6-tailnet-1440-auto-fgs-workload-binding.json`: 최신 설치본 재측정. 판정 FAIL.
- `runtime-input-resume-*.log`, `runtime-input-actions.jsonl`: ADB 입력과 백그라운드 복귀 실패, 전송 EPERM 근거.
- `connection-service-installed-receipt.json`, `connection-service-runtime-receipt.json`, `fgs-resume-*.jsonl`: foreground service 수정본 설치와 73초 백그라운드 후 실제 영상 복귀.
- `window-close-native.log`, `window-close-status.jsonl`, `window-close-service-final.txt`: 메인 창 제거 후 영상 유지, 마지막 창 종료 후 서비스·Host 세션 정리.
- `vpn-lan-comparison-receipt.json`: VPN 일시 해제, LAN 경로와 연결, VPN 복원 근거.
- `s2-tailnet-1440-custom16-exclusion.json`: 비교 제외 사유.
- `udp-mtu-*.log`, `mtu-host-candidate-receipt.json`, `mtu-host-install.json`: 패킷 수정 및 설치 증거.
- `reconnect-installed-receipt.json`, `normal-restart-approval-receipt.json`: 새 Host 설치와 실제 정상 재시작 시 승인 유지.
- `cache-cleanup-receipt.json`: 정리 범위와 전후 공간.
- `current-validation-status.json`: 통합 상태.

기준은 1440p 이상, 180초 이상 고동작, 60fps 목표, 샘플 90% 이상에서 57fps 이상, 지연 p95 50ms 이하, RTT p95 20ms 이하, 새 gap·복구·송신 실패·하한 압박 0이다. Surface release는 실제 패널 발광 시점이 아니고 ADB 입력은 물리 Caps Lock/트랙패드와 같지 않다. Parsec과 동일 조건 비교는 아직 없다.

참고: [Tailscale 직접 연결 성능](https://tailscale.com/docs/reference/best-practices/performance), [MTU 1280](https://tailscale.com/docs/reference/troubleshooting/network-configuration/tcp-connection-two-devices).

연결 유지 서비스 유형: [Android connectedDevice foreground service](https://developer.android.com/develop/background-work/services/fgs/service-types#connected-device).
