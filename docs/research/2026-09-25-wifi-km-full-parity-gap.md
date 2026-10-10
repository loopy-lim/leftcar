# Wi-Fi 양방향 KM — 완전 패리티 격차 조사 (full-parity-gap)

문서 상태: 조사 결과 + 설계 정정 근거. 기준일 2026-09-25. 구현 코드 변경 없음.
근거 JSON: [2026-09-25-wifi-km-full-parity-gap.json](2026-09-25-wifi-km-full-parity-gap.json).
정정 대상: `docs/superpowers/specs/2026-09-25-wifi-bidirectional-km-design.md`, `docs/superpowers/specs/2026-09-25-wifi-bidirectional-km-acceptance.json`.

## 1. 발견된 격차 (gap)

최종 리뷰에서 기존 설계가 다음 세 가지로 사용자의 원래 목표를 **조용히 축소**했음이 확인됐다:

1. **보이지 않는 커서**: Mac → Android에서 "커서 없이 조작 중"을 v1 제품 계약화(Q3의 무답변 시 "수용으로 간주" 기본값 포함).
2. **Android → Mac 키보드 전용 토글**: 태블릿 마우스 방향은 배제되고 진입도 명시 토글로 제한(Q5).
3. **Android 경계 이탈 보류**: edge crossing의 절반(Android→Mac 경계 트리거)을 v1.1로 연기.

사용자의 원래 의도는 Universal Control 수준의 **양방향 마우스·키보드 경계 이동**이다. 축소된 설계는 그 목표를 만족하지 못하며, 본 조사는 그 축소가 정당했는지 — 특히 "root/uinput만이 완전 커서 경로"라는 핵심 주장 — 를 1차 자료로 검증했다.

**판정: 주장은 거짓.** 무선 ADB(adb shell 권한)로 만들 수 있는 UHID 가상 HID 장치가 root·USB 없이 실제 커서·hover·전체 HID 키보드를 제공한다. 설계는 이를 반영해 정정됐다.

## 2. 1차 근거: scrcpy UHID + 무선 ADB

### 2.1 scrcpy 공식 문서 (실증 제품)

- `--mouse=uhid`: "simulates a physical HID mouse using the UHID kernel module on the device". 물리 마우스 시뮬레이션 모드(UHID·AOA)에서는 "the mouse pointer disappears from the computer and **appears on the Android device instead**" — 즉 **Android OS가 렌더링하는 실제 커서**다. 커서가 "장치 측에서 처리"되므로 hover도 네이티브로 발생한다.
- `--keyboard=uhid`: "it works for all characters and IME (contrary to `--keyboard=sdk`)"이고, 결정적으로 "**it works over TCP/IP (wirelessly) (contrary to `--keyboard=aoa`)**".
- AOA만이 "works at the USB level directly (so it only works over USB)" — USB 전용은 AOA이고, UHID는 무선 ADB에서 동작한다.
- 주의: "UHID may not work on old Android versions due to permission errors" — 구형 Android에서는 `/dev/uhid` 권한 오류 가능. **기기 실측 필요 항목**이다.

### 2.2 scrcpy 서버 소스 (`UhidManager.java`)

adb로 실행된 scrcpy 서버(= **shell uid**, root 아님)가:

```java
FileDescriptor fd = Os.open("/dev/uhid", OsConstants.O_RDWR, 0);
...
Os.write(fd, req, 0, req.length);   // UHID_CREATE2 — 가상 HID 장치 생성
Os.write(fd, req, 0, req.length);   // UHID_INPUT2 — HID 리포트(마우스/키보드 이벤트) 주입
```

root 권한 요소가 전혀 없다. 권한 경계는 "root 여부"가 아니라 "`/dev/uhid` 파일 접근 + SELinux 도메인 허용"이다.

### 2.3 AOSP 권한 경계 (`shell.te`, `ueventd.rc`)

`system/sepolicy/private/shell.te`:

```
# allow shell input injection
allow shell uhid_device:chr_file rw_file_perms;
```

AOSP가 **의도적으로** adb shell 도메인에 UHID 장치 생성을 허용한다(주석 자체가 "allow shell input injection"). 같은 파일의 반대 경계도 명확하다:

```
allow shell input_device:chr_file r_file_perms;      # 기존 물리 입력 장치 읽기는 허용
neverallow shell input_device:chr_file no_w_file_perms;  # 쓰기(억제)는 금지 — "abuse vector"
```

정리하면 shell uid(무선 ADB 포함)는:

| 능력 | 가능 여부 | 근거 |
| --- | --- | --- |
| `/dev/uhid`에 가상 HID 마우스·키보드 생성(실제 커서·hover·전체 HID) | **가능** | scrcpy 문서+소스, `shell.te` allow |
| 기존 물리 `/dev/input` 노드 **읽기**(전역 관찰 후보) | 허용(미검증) | `shell.te` r_file_perms |
| 기존 물리 `/dev/input` 노드 **쓰기**(물리 입력 억제) | **불가(neverallow)** | `shell.te` neverallow |
| 프레임워크 수준 전역 모션 이벤트 필터 | 불가(공개 API 부재) | AccessibilityService는 키 필터만 제공 |

## 3. 정정된 방향 매트릭스

| 항목 | Mac → Android | Android → Mac |
| --- | --- | --- |
| **마우스** | 접근성-only: 커서 없는 제스처(PARTIAL). **무선 ADB+UHID: 실제 커서·상대 이동·휠 완전**(승인·실측 조건부) | Leftcar 캡처 한정(구현됨). 전역 관찰은 evdev 읽기/오버레이 스트립 후보 — 미검증 |
| **키보드** | 접근성-only: IME+globalAction 부분집합. UHID: 전체 문자·IME 커버 | 접근성 키 필터로 전역 관찰·억제·전송 — **사실상 완성**(기존 KeyBridge 경로) |
| **경계 이동(edge crossing)** | Mac 측 CGEventTap 트리거 + UHID 가상 커서 좌표를 자체 데몬이 소유 → 양방향 경계 이동 구현 가능(미실측). 접근성-only는 Android 측 트리거 없어 토글 폴백 | 키보드는 상시 가능. 마우스는 (a) Leftcar 캡처 중 이탈(구현), (b) 스트립, (c) evdev 관찰 — (b)(c) 기기 실측 전까지 미충족으로 기록 |
| **전역 캡처·억제** | Mac 측: CGEventTap defaultTap 가능. Android 측 물리 마우스 억제: 불가(모션 필터 부재 + /dev/input 쓰기 neverallow). Android 물리 키보드 억제: 접근성 필터 가능 | 키보드 YES(접근성), 마우스 NO(공개 API·shell 쓰기 모두 불가) |
| **커서·hover** | 접근성-only 없음 / UHID 실제 커서·hover | Mac 측 커서는 CGEvent 주입의 진짜 macOS 커서(완전). 미충족은 Android 측 관찰뿐 |
| **상태** | 완전 패리티 후보 존재 — **사용자 ADB 승인 + 기기 실측 전까지 blocked** | 키보드 준비 완료, 마우스 PARTIAL — 실측 전 |

## 4. Wi-Fi 권한 옵션 (사용자 결정 입력)

| 옵션 | 부여 권한 | 커버 | USB | 수명 | 역할 |
| --- | --- | --- | --- | --- | --- |
| **P1 접근성 only** | 접근성 서비스 활성화(설정 1회) | 키보드 Android→Mac 전역, Mac→Android는 커서 없는 제스처·텍스트 | 불필요 | 재부팅 유지 | **축소 폴백** — 사용자 명시 수용 시에만 |
| **P2 무선 ADB + UHID** | 개발자 옵션 + Android 11+ 무선 디버깅 페어링(Wi-Fi) | 실제 커서·hover·양방향 경계 이동·전체 HID 키보드 | 불필요(무선 페어링) | 무선 디버깅 토글의 재부팅 생존은 버전·OEM 의존(TB710FU 실측 필요) | **완전 패리티 후보** — 승인 전 |
| **P3 root/시그니처** | root 또는 플랫폼 서명 | 전체 | — | — | 제외 유지. 단 "유일 경로" 문구는 P2가 반증하여 삭제 |

P2의 남은 미지수: (1) TB710FU의 `/dev/uhid` 접근 허용 여부(구형 Android 권한 오류 사례), (2) 무선 ADB 상주 데몬(app_process)의 주입 지연이 커서 사용감을 만족하는지, (3) 무선 디버깅 재부팅 생존, (4) OEM 배터리 최적화·Android 13+ restricted settings의 영향.

## 5. 결론

1. **"root/uinput 유일" 주장은 철회된다.** 무선 ADB + UHID가 root·USB 없는 완전 커서 경로다(scrcpy 실증 + AOSP 정책 명시).
2. **원래 목표(양방향 실제 커서·키보드 경계 이동)는 만족되지 않았다** — `original_goal_satisfied: false`. 접근성-only로의 강등은 무효화하고, P2 승인 여부가 다음 사용자 결정이다.
3. **물리 테스트는 수행되지 않았다** — `physical_tests_passed: false`. 승인 후에도 TB710FU 실측(UHID 권한, 데몬 지연, 경계 전환 레이턴시)이 선행 조건이다.
4. 키보드 Android→Mac 방향만이 기존 접근성 경로로 사실상 준비 완료다. 나머지는 P2 승인 + 실측에 매달린다.
5. 접근성 축소 모드는 폴백으로 유지되며, 사용자의 명시적 수용 없이는 기본 계약이 될 수 없다.
