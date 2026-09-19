# IME 전환과 Tailscale 직접 접속

## 현재 결론

후속 상태: 사용자가 외부에 있어 Lenovo 실기와 추가 설치를 보류했다. 4K 복귀의 자동 재준비 수정과 호스트 단독 pacing 비교는 [실기 보류 중 작업 기록](2026-09-16-offline-followup.md)을 따른다. 아래 설치 해시는 마지막 기기 적용본이며, 후속 후보는 아직 설치하지 않았다.

- Lenovo 두 손가락 스크롤은 수정본에서 사용자 실기 확인을 받았다. Android 분류 3 이벤트의 휠·제스처 거리 축은 0이지만 좌표 변화는 존재했다.
- Caps Lock 조작 때 Android의 실제 IME subtype이 한국어/영어로 바뀌는 것을 기록했다. 기존 Leftcar 키 이벤트에는 전환이 전달되지 않았다.
- Tailscale IPv4 영상은 실기로 확인했다. 세션 4에서 Viewer `100.77.109.50:5005`, 2560×1440 영상 출력과 입력 활성 상태가 기록됐다. 제어 접속만 성공한 상태와 구분한다.
- 첫 IME 설치본에서 사용자가 한·영 전환 실패와 다른 앱에서 복귀 후 영상 끊김을 보고했다. 아래 두 원인을 수정한 두 번째 설치본을 적용했다. **수정 후 물리 Caps Lock 전환과 화면 복귀는 사용자 확인 대기다.**
- 같은 Wi-Fi 위의 Tailscale 경로 확인과 집 밖 다른 네트워크 검증은 별개다. 외부 WAN 실기는 수행하지 않았다.

## 사용자 설정과 원인

태블릿 KeyBridge는 `[caps] tap = "language"`, `[android] language_switch = "language_switch"`였다. Mac 이전 입력 소스는 Ctrl+Space, 다음 소스는 F17/Fn 단축키였다. 사용자 설정을 변경하지 않았다.

Gboard 한국어 subtype은 `ko-KR`이며 물리 키보드 힌트는 `en`이다. Alphabet 영어 subtype은 언어 태그/locale이 비어 있고 물리 키보드 힌트가 `en-Latn-US`다. 한국어 `-1906255757`과 영어 `-1097284911`이 물리 Caps Lock 조작 때 여섯 번 교차한 기록이 있다. 안내 문구를 읽을 필요 없이 [공개 IME subtype API](https://developer.android.com/reference/android/view/inputmethod/InputMethodManager#getCurrentInputMethodSubtype())로 상태를 읽는 경로를 사용했다.

Tailscale을 켠 직후 두 종류의 실패를 관찰했다.

1. 기존 `192.168.0.134:7777` 제어 연결은 Android Tailscale이 오프라인인 서브넷 경유 장치로 보낸 뒤 시간 초과했다.
2. `100.80.133.120:7777` 제어 연결은 성공했지만 `100.77.109.50` 영상 포트의 UDP 도달 증명에 실패했다. Viewer는 Host가 광고한 LAN 주소를 영상의 기대 피어로 사용하고 있어 Tailscale 발신 주소를 거부했다.

Tailscale 상태 확인 시 Lenovo `100.77.109.50`은 온라인이며 Host에서 직접 경로 응답이 약 12ms였다. 이후 `tailnet-live-status.jsonl`에서 1440p 실제 영상이 확인됐다. 이 기록에는 프레임 복구도 존재하며 고부하 성능 합격을 의미하지 않는다.

첫 IME 구현의 `LCL1` 지원 응답은 UDP 목적지 인자를 빠뜨렸다. Host에서 전송하지 않았으므로 Viewer는 지원 여부를 확인하지 못해 언어 변경을 보내지 않았다. 응답 바이트를 실제로 수신하는 테스트를 추가해 수정 전 실패를 확인하고 목적지를 전달하도록 수정했다. 초기 `LCS1` 입력 상태 통지도 같은 누락을 수정했다.

단일 영상의 Surface가 사라지면 디코더와 프레임 피드백이 멈췄다. Host는 5초 후 연결 유실로 판단했고, 복귀 때 새 렌더러는 이미 소비된 미디어 키를 다시 가져올 수 없었다. 이 경로에서 화면과 연결의 수명을 분리했다.

12:58의 앱 종료 기록에는 TCP 라이브러리의 `No socket with id 15`가 있었다. 연결 정리 작업이 실행될 때 클라이언트가 없으면 예외가 작업 스레드 밖으로 전파됐다. 반복 종료 및 큐 대기 중 제거를 실제 라이브러리로 재현한 뒤, 없어진 클라이언트의 종료를 안전하게 무시하는 영구 의존성 패치를 추가했다.

## 구현

### 실제 입력 언어 동기화

- Android Activity가 재개된 동안 IME 설정 변경을 관찰하고 500ms 간격으로 누락을 보완한다. 키 입력 직전에도 현재 언어를 다시 읽는다.
- 언어 태그 → locale → 물리 키보드 힌트 순서로 해석한다. 알려지지 않은 언어를 영어로 오인하지 않는다.
- 포커스 상실, 일시 중지, 종료, 원격 입력 잠금에는 전송하지 않는다. 연결 재생성/포커스 복귀 때 상태를 다시 동기화한다.
- 암호화된 기존 미디어 제어 경로에서 `LCL?` / `LCL1`으로 Host 지원을 확인한다. 구버전 Host에는 새 신뢰 입력 이벤트를 넣지 않아 키 입력 큐를 막지 않는다.
- `LCI1` kind 7의 한 바이트 언어 값(영어 1, 한국어 2)을 기존 재전송/ACK 큐로 보낸다. Mac에서 전환이 끝나야 ACK하므로 다음 글자보다 먼저 적용한다.
- Mac은 메인 스레드에서 TIS API로 이미 활성화된 기본 입력 소스를 선택한다. 같은 언어의 현재 레이아웃은 유지하며 단축키/키보드 설정을 변경하지 않는다. 중복, 입력 잠금, 포커스 release-all, 세션 종료 시 대기 중인 전환을 처리한다.
- 현재 동기화 대상은 영어/한국어다. 소프트 키보드의 확정 문자열 전송 경로는 그대로 사용한다.

### Tailscale 영상 경로

- 제어 연결의 실제 숫자 피어 주소를 보존한다. 선택된 Tailscale IPv4/호스트명 경로는 카탈로그에 있는 LAN 주소로 대체하지 않는다.
- Host는 Tailscale 제어 피어에 대해 Viewer가 별도로 광고한 LAN 주소를 먼저 시도하지 않는다. 인증된 제어 피어의 주소를 영상 대상으로 사용한다.
- LAN 같은 /24 후보와 USB 경로는 기존 동작을 유지한다. 수신 피어 검사 및 미디어 암호화를 완화하지 않았다.
- 직접 IP 입력 UI는 원래 존재한다. 현재 실기 대상은 Tailscale IPv4이며 MagicDNS/IPv6 실기 성공을 주장하지 않는다.
- 공인 IP 허용 목록이나 공유기 포트 전달 설정은 변경하지 않았다.

### 화면 복귀와 연결 정리

- 현재 1440p 단일 Surface 경로는 기존 소켓·암호 상태·신뢰 입력 순서를 보존하고, 디코더의 중지 확인 후 새 Surface만 연결한다. 새 스트림 키가 준비된 경우와 다른 소유자·해상도·호스트에는 이 재사용을 적용하지 않는다.
- 화면이 숨겨진 동안 암호화된 `LCK1` heartbeat로 연결을 유지하고, 키 해제 입력과 ACK도 처리한다. 가짜 프레임 피드백을 보내지 않는다. 복귀 때 디코더와 지연 감시 기준을 새로 시작하고 IDR을 요청한다.
- 실제 네트워크 단절·앱 종료에서는 heartbeat도 끊기므로 Host의 연결 정리 기능을 유지한다.
- 4K 분할 디코더는 별도 수명 경로다. 후속 후보는 Surface 복귀/붙이기 실패 때 같은 창에서 Host 연결을 다시 준비하도록 수정했고 JVM 테스트를 통과했다. 해당 후보는 설치하지 않았으며 물리 복귀 검증은 남아 있다.
- TCP 종료 수정은 `patches/react-native-tcp-socket@6.4.2.patch`와 Bun lockfile에 저장했다. 오래된 isolated 심볼릭 링크가 hoisted 패치를 우회하는 것을 발견해 해당 생성 링크와 자동 연결 캐시를 갱신했고 실제 빌드 참조 경로를 확인했다.

## 검증 및 설치물

- Android 전체 단위 테스트 131개 통과, 일반 APK 빌드 성공.
- Rust Android Viewer 테스트 260개 통과.
- Swift 실제 수신 처리 테스트: 전환 전 ACK 보류, 중복 전환 방지, release-all 취소, 잠금 상태 큐 소진 통과. 실제 암호화 UDP의 `LCL1`·`LCS1` 응답 수신도 확인했다. OS 입력 소스 변경은 테스트 대역으로 분리했다.
- Tailscale 경로 회귀 테스트는 수정 전 실패, 수정 후 통과. React Doctor **100 / 100**, 전체 TypeScript 검사 및 최신 관련 TypeScript 테스트 59개 통과.
- IME/복귀/TCP 종료 수정 설치 APK SHA-256: `a1a2aec26916f6c1d629c0f7b4273c597a7fe73b69caeec9a5b1eba3020ed0ce` (기기 파일 읽기 대조 일치).
- 설치 Host 실행 파일 SHA-256: `661b1cb9461159b0bd53fce6fc5061f516f026d3bef0f51675dfc08e2ccc8fe7`.
- 설치 capture shim SHA-256: `0a12cc880083c4cc68dd77b5a831e00adba9f860929c91aad6e047a7f606c3db`.
- 동일 서명 검증 후 `/Applications/Leftcar Host.app`에 적용했다. 이전 앱은 `.previous-f456a208-b135-473e-8179-98f9aa6ca17b`에 보관했다.
- TCP 종료 패치까지 포함한 APK 빌드·설치·기기 파일 해시 대조를 완료했다.

원시 근거는 `artifacts/performance-acceptance-2026-09-16/`의 `physical-ime-r2.jsonl`, `physical-input-r2.log`, `ime-tailnet-installed-receipt.json`, `ime-resume-installed-receipt.json`, `ime-resume-tcp-installed-receipt.json`, `ime-resume-checks.json`, `tailnet-live-status.jsonl`, `tcp-close-red.log`, `*-ime-*.log`, `tailnet-regression-tests.log`에 있다. 입력한 문자열은 진단 로그에 기록하지 않는다.

## 실기 확인 대기

1. 최신 수정본에서 Tailscale `100.80.133.120:7777` 영상과 입력 재연결. 이전 설치본에서 실제 1440p 영상은 확인했다.
2. Caps Lock으로 한국어/영어를 바꾼 뒤 실제 조합/영문 타이핑, 다시 전환. 다른 앱으로 10초 이상 이동 후 복귀.
3. 같은 후보에서 1440p 이상 전체 화면 고부하 성능 측정. 기존 성능 실패 결과를 새 후보의 통과로 대체하지 않는다.
4. 가운데 클릭은 단위 테스트만 통과했으며 물리 휠 누르기 성공 근거는 별도로 필요하다.
