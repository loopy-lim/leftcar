# USB ADB 진단 및 태블릿 Leftcar 세션 복원 검증 (2026-10-03)

## 요약

- **USB ADB 진단 채널**: ✅ 성립 — 물리 USB로 TB522FU(LenovoTab LEGION Y900 13, serial 마스킹 `HA2H***D`)가 열거되고, `adb devices`에서 `device usb:1-1`(authorized)로 인식. 무선 ADB(`adb connect/pair/tcpip`)는 전혀 사용하지 않았고 네트워크 serial도 없음 → **selected_transport=usb**.
- **실제 Leftcar 세션**: ✅ **restored / connected** — 설치된 Viewer를 실행하고 일반 앱 UI 흐름(mDNS 발견 → 연결 → Display 1 열기)만으로 기존 호스트와 직접 Wi-Fi 세션(session 11, UDP) 재성립. 신규 `session_started` 레코드 + 렌더 프레임 지속 증가로 fresh session·media progress 모두 검증.
- **양방향 full KM**: ❌ 미충족 — 이번 작업으로도 구현·검증되지 않음 (`full_km_satisfied=false`).
- **제품 USB 데이터 전송**: ❌ 미구현 — USB는 이번에도 진단·실행 채널이며 제품 전송로는 직접 Wi-Fi (`product_usb_transport_implemented=false`).

## 대상 기기 식별 (이전 run과 다른 태블릿)

| 항목 | 이전 run (무선 ADB) | 이번 run (USB ADB) |
|---|---|---|
| 모델 | TB710FU | **TB522FU** |
| USB 제품명 | — | LenovoTab LEGION Y900 13 |
| serial | HA2D*** | **HA2H***D** |
| 전송 | `192.168.0.18:37727` (TLS 무선) | **`HA2H***D device usb:1-1`** |
| 잠금 | deviceLocked=1 (blocked) | **deviceLocked=0** (해제 상태) |

두 기기는 서로 다른 태블릿이며, 이번 USB 기기에는 Viewer(`leftcar.ll3.kr`)가 이미 설치되어 있었다.

## USB 양성 구별 증거

```text
$ ioreg -p IOUSB -w 0 -l | grep -A25 LenovoTab
  "USB Product Name" = "LenovoTab LEGION Y900 13"
  "USB Vendor Name"  = "LENOVO"
  "USB Serial Number" = "HA2H8QSD"   (문서에서는 마스킹)
  UsbLinkSpeed = 480000000           (USB 2.0 High-Speed, 데이터 라인 활성)

$ adb devices -l
HA2H8***SD  device usb:1-1  product:TB522FU_PRC  model:TB522FU  transport_id:1
```

- serial이 IP:port 형태가 아니고 `usb:1-1` 표기 → USB transport 양성.
- `adb kill-server && adb start-server` 후에도 USB transport만 존재.
- `adb connect`·페어링·`tcpip` 명령 0회 사용.

## 진행 타임라인 (주요 명령 → 결과)

| 시각(KST) | 명령 | 결과 |
|---|---|---|
| 21:00 | `ioreg -p IOUSB` / `adb devices -l` | USB 열거 + authorized USB ADB 확인 |
| 21:01 | `pm list packages`, `dumpsys trust` | Viewer 설치 확인, **deviceLocked=0** |
| 21:02:39 | `python3 tools/leftcar-live-connection-check.py` | **before: disconnected** (host 정상, fresh session 없음, age≈18.4h) |
| 21:02:50 | `am start -n leftcar.ll3.kr/dev.leftcar.viewer.MainActivity` | Viewer 포그라운드 (잠금 장벽 없음) |
| 21:03:10 | UI 탭: "지금 화면 열기" | 저장된 호스트 192.168.0.50 stale → "컴퓨터가 응답하지 않습니다" |
| 21:03:36 | `dns-sd -B _leftcar._tcp local.` | `leftcar-host` mDNS 광고 정상 |
| 21:03:45 | UI 탭: "+ 다른 컴퓨터 연결" → mDNS 항목 `leftcar-host (192.168.0.154)` 연결 | "연결된 컴퓨터: 192.168.0.154:7777" 화면 선택 도달 |
| 21:04:00 | UI 탭: Display 1 "열기" | OS 권한 대화상자 없이 StreamActivity 진입(기존 승인 재사용) |
| 21:04:11 | checker 재실행 | **verdict=connected**, fresh_active_session=True (age 11s) |
| 21:04:20 | `logcat --pid=<viewer> \| grep Rendered` | Rendered 1410→2160 (~55fps), outputDrops=0, inputRtt≈8ms |
| 21:04:51 | `adb devices -l` + checker | **after: USB 단독 transport + connected (age 39.2s)** |

## 세션·미디어 증거

```text
$ tail -1 ~/Library/Application Support/leftcar-host/sessions.jsonl
{"session":11,"device":"dev:34d4***4999","transport":"udp","ts":"unix:1791029052","event":"session_started"}

$ python3 tools/leftcar-live-connection-check.py
verdict: connected
  host_process: True
  host_tcp: True
  adb_device: True
  fresh_active_session: True
  session_age_sec: 39.2
```

Viewer logcat (`LeftcarNative`): `Rendered 1410 → 1950 → 2160 frames` (약 10초, ~55fps),
`outputDrops=0`, `nacksSent=0`, `inputRttMs=Some(8)`, `outputPtsUs` 지속 증가, `stream=5001`.

## 제품 경계 유지

- **제품 기본 전송로**: 앱 간 직접 Wi-Fi — 제어 TCP 7777(호스트 192.168.0.154), 미디어 UDP. USB ADB는 진단·실행·UI 관찰 채널일 뿐이다.
- **무선 ADB 제품 의존 없음**: 이번 세션은 무선 ADB 없이 성립 — 이전 run의 무선 ADB 의존 우려를 해소.
- **full KM (양방향 키보드/마우스)**: 미구현·미검증 유지. 이번 connected는 기존 스트리밍·단방향 입력 연결에만 적용.
- **제품 USB 전송**: 미구현. 향후 확장 시 `apps/viewer-expo/src/usb*.ts`와 Android USB 서비스 측 구현이 필요.
- 금지 항목 준수: keyguard 우회, 보안 설정 변경, 앱 데이터 삭제, 앱 설치, RSA/권한 프롬프트 자동 승인 없음. 이번 run에서는 잠금 해제 상태라 OS 동의가 필요한 순간 자체가 없었다.

## 남은 과제 / 다음 사람 행동

즉시 필요한 사람 행동은 없다. 후속 항목:

1. 양방향 full KM 구현 및 실기기 검증 (별도 작업).
2. 제품 USB 데이터 전송 구현 (선택적 미래 확장).
3. Viewer에 저장된 stale 호스트 IP(192.168.0.50, 100.80.133.120) 정리 UX — 이번 run에서 mDNS 재발견으로 우회 성공.
