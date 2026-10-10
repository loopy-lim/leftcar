# ADB Leftcar 라이브 연결 검증 (2026-10-03)

## 한 줄 요약

**무선 ADB 연결 성공(TB710FU, 192.168.0.18:37727) → 진단 완료. 단, 태블릿 잠금 화면이 Viewer를 가려 세션 복구는 blocked.**

다음 사람 행동: **TB710FU 태블릿을 잠금 해제**하면 즉시 재시도 가능. 잠금 외에는 모든 경로(호스트 listen, mDNS, LAN 도달, ADB 인증)가 정상 확인됨.

## 배경과 제약

- 사용자 지시: "adb로 연결해서 진행" — 이미 페어링된 무선 ADB(mDNS `_adb-tls-connect._tcp`)를 진단·Viewer 실행·일반 앱 상호작용에만 사용.
- 금지 준수: 무선 디버깅 설정 변경 없음(이미 켜져 있던 서비스에 접속), 페어링 시크릿 추출 없음, root/tcpip/앱 데이터 삭제/설치 없음, **OS 잠금 우회 없음**.
- ADB는 진단 채널일 뿐 제품 전송로가 아님. 제품 데이터 통신은 앱 간 직접 Wi-Fi(제어 TCP 7777 + UDP 미디어).

## 관측 결과

| 계층 | 상태 | 증거 |
|---|---|---|
| ADB | ✅ 연결 | `adb connect 192.168.0.18:37727` → `device` (TB710FU_PRC, Android 16) |
| 호스트 프로세스 | ✅ 실행 중 | `leftcar-host-desktop` PID 67539 |
| 제어 평면 | ✅ 대기 중 | TCP `*:7777` listen + mDNS `_leftcar._tcp leftcar-host` 광고 |
| LAN 도달성 | ✅ 정상 | Mac(192.168.0.133) ↔ 태블릿(192.168.0.18) ping 0% loss |
| **라이브 세션** | ❌ **없음** | 마지막 session_stopped `unix:1790963168` (02:46 KST, 약 18.3시간 경과) |

## before / after

- **before (20:55 KST): `disconnected`**
  - `sessions.jsonl`에 02:46 이후 `session_started` 없음
  - Viewer 프로세스 생존(PID 13230), MainActivity가 task top이나 화면에는 도달 못함
- **after (20:57 KST): `blocked`**
  - 잠금 해제 폴링 12회(약 2분) 동안 `deviceLocked=1` 유지
  - keyguard `mIsShowing=true, inputRestricted=true`
  - Viewer `onHostResume → onUserLeaveHint → onHostPause` 즉시 전환 (logcat --pid=13230)

## 진단 체인 (가설 배제 과정)

1. **ADB 미연결 가설** → 기각. mDNS로 무선 ADB 서비스 발견, connect 즉시 성공.
2. **호스트 데드 가설** → 기각. 프로세스·포트·mDNS 모두 생존.
3. **LAN 단절 가설** → 기각. ping 0% loss, 같은 서브넷.
4. **Viewer 미실행 가설** → 기각. 프로세스·태스크 존재, `am start` 응답.
5. **잠금 병목 가설** → **확증.** `dumpsys trust/window/policy` + logcat의 즉시 pause 패턴이 일치. discriminating probe로 분류 완료.

## 사용한 명령 (요약, 비밀 정보 없음)

```
adb devices -l
dns-sd -B _adb-tls-connect._tcp local.
dns-sd -L adb-HA2D6EMP-Qnp1Nz _adb-tls-connect._tcp local.
dns-sd -G v4 Android.local.
adb connect 192.168.0.18:37727
adb -s 192.168.0.18:37727 shell getprop ro.product.model
adb -s 192.168.0.18:37727 shell dumpsys trust / window / power
adb -s 192.168.0.18:37727 logcat -d --pid=13230
adb -s 192.168.0.18:37727 shell am start -n leftcar.ll3.kr/dev.leftcar.viewer.MainActivity
python3 tools/leftcar-live-connection-check.py
```

## 체커

`tools/leftcar-live-connection-check.py` — 표준라이브러리 전용, 읽기 전용.

```bash
python3 tools/leftcar-live-connection-check.py --help
python3 tools/leftcar-live-connection-check.py --self-test   # 합성 disconnected 실패 / connected 통과
python3 tools/leftcar-live-connection-check.py               # 실측: verdict=disconnected (exit 1)
```

`connected` 판정은 **신선한 앱 수준 세션 레코드**(`--fresh-window` 이내 `session_started`, 이후 stop 없음)가 있을 때만 나온다. 프로세스·포트 생존만으로는 성공 판정하지 않는다.

## 코드 변경 필요 여부

**불필요.** 이번 차단은 OS 잠금 상태 문제이지 제품 결함이 아니다. 만약 잠금 상태에서도 백그라운드 재연결 시도 같은 UX 강화가 원해지면 소유 위치는 `apps/viewer-expo/src`(연결 훅)와 `apps/viewer-expo/android/app/src/main`(NSD/포그라운드 서비스)이다.

## 남은 전체 목표 기준 (full KM)

`full_km_satisfied=false`. 이번 작업은 **기존 스트리밍·입력 연결의 복구**만 다룬다.

- 양방향 KM(Mac→Android 커서 주입 포함): 미구현·미검증
- 서비스 배선·macOS 권한(ListenEvent): 미완
- 양방향 경계 실기기 검증: 미수행
- USB 전송 확장: 보류 (직접 Wi-Fi가 기본)

상세는 `docs/research/2026-10-03-adb-leftcar-live-connection-validation.json` 참조.
