# 가상(확장) 화면 종료 수명주기 감사 — 2026-09-22

목적: 4a68d15(`fix(display): 가상 화면 수명과 배치 입력 좌표 수정`)의 ARC 소유권 수정 이후에도
"화면 제거 명령·세션 종료·앱 종료 시점에 확장 화면이 꺼지지 않는다"는 증상이 남는 근본 원인을
호출 그래프로 확정한다. 본 문서는 리서치 결과만 담는다(코드 변경 없음).

감사 범위: `apps/host-desktop/src-tauri/src/**`, `apps/host-desktop/src/**`,
`native/macos-capture-shim/Sources/**`, `native/macos-capture-shim/Tests/**`,
`docs/2026-09-18-extended-display-design.md`, `docs/superpowers/specs/2026-09-22-sidecar-extension-verification.md`.

---

## 1. 수명주기 레이어와 소유 모델

```
[호스트 UI / 태블릿 뷰어]
   │ Tauri command / control 채널 명령
   ▼
VirtualDisplayManager (Rust, src-tauri/src/virtual_display.rs)
   │  state.current = 논리 소유 레코드, state.last_removed = 제거 지연 추적
   │ dlopen 공유 심볼
   ▼
CGVirtualDisplayBridge.m (ObjC, dylib 내부)
   │  VDRegistry: displayID → CGVirtualDisplay 강한 참조 (:132-139, :388)
   │  레지스트리 보유 = 디스플레이 수명. removeObjectForKey = ARC release = 제거 (:502-506)
   ▼
CGVirtualDisplay (비공개 CoreGraphics 클래스) → WindowServer
```

- 소유 정책은 **호스트 소유 영속**이다: 세션·시청자 수명과 무관하게 호스트 프로세스가 만들고 없앤다
  (`docs/2026-09-18-extended-display-design.md` §6 결정 1, :74 "disconnect → destroy / grace … grace 타이머 자체가 불필요";
  `docs/superpowers/specs/2026-09-22-sidecar-extension-verification.md` "스트림 종료나 네트워크 단절은 Mac 작업 공간을 삭제하지 않는다").
- 파괴 API는 별도 없다. "객체 해제 = 제거"가 동작 모델이고, 제거는 비동기다(실측 최대 ~30s, design.md :231).

---

## 2. 생성 경로 전수

| # | 진입 | 호출 경로 (파일:행) | 비고 |
|---|---|---|---|
| G1 | 호스트 UI "만들기/적용" | `ExtendedDisplayCard.tsx:105` → `virtual_display_create` (`lib.rs:884`) → `begin_virtual_display_change` (`lib.rs:886`) → `manager.create` spawn_blocking (`lib.rs:897`) → `create_inner` (`virtual_display.rs:349`) → `call_create` (`virtual_display.rs:530`) → `leftcar_vdisp_create_v1` (`CGVirtualDisplayBridge.m:273`) | UI 2초 폴링: `ExtendedDisplayCard.tsx:133` |
| G2 | 태블릿(뷰어) 확장 화면 열기 | `use-extension-display.ts:77` → control 명령 `createVirtualDisplay` (`control.rs:2448`) → `authorize_virtual_display(..., true)` (`control.rs:2451`, 정의 `control.rs:710`) → spawn_blocking `manager.create` (`control.rs:2467-2510`) → 동일 브리지 | 물리 메트릭 자동 매칭(`match_display_size`) 또는 명시 mode. 뷰어 메트릭은 `viewer_metrics` (`control.rs:2524-2531`) |
| G2' | 뷰어 재열기(이미 존재) | create가 "already exists"로 실패하면 `status()`로 현재 디스플레이 반환 (`control.rs:2505-2515`) | 재생성이 아니라 재사용 |
| G3 | 리사이즈(내부적으로 destroy→create) | 호스트: `virtual_display_resize` (`lib.rs:866`) / 뷰어: `use-extension-display.ts:99` → `resize_virtual_display` (`control.rs:674`) → `VirtualDisplayManager::resize` (`virtual_display.rs:433`) → `remove_inner`(`:450`) → `create_inner`(`:459`) | 같은 identity 재생성 |

브리지 내부 생성 순서 (`CGVirtualDisplayBridge.m:273-394`): 검증 → 중복 가드(`:297-300`, 레지스트리 기준) →
descriptor/mode/settings 구성(ARC init family 주석은 4a68d15에서 추가, `:29-35`) → `initWithDescriptor:`(`:343`) →
`applySettings:`(`:346`) → 활성/모드 폴링(`:359-381`) → **성공 시에만** 레지스트리 등록(`:386-389`).

---

## 3. 꺼져야 하는 시점 × 실제 destroy 호출 여부

호출 그래프 (제거가 일어나는 유일한 형태):

```
manager.remove() (virtual_display.rs:411)
  └─ remove_inner (:417)
       ├─ state.current 없음 → no-op Ok (:420-421)   ← [C2 위험, §5]
       ├─ call_destroy (:578) → leftcar_vdisp_destroy_v1 (bridge.m:502)
       │    ├─ 레지스트리 없으면 rc=1 (:503-504)      ← Err → current 유지
       │    ├─ removeObjectForKey (:505-506) = ARC release = 유일한 해제 지점
       │    └─ 활성 목록 폴링 1.5s → 빈 재구성 sweep + 8s 폴링 (:508-516)
       │         rc: 0=소실, 2=해제됐으나 WindowServer 열거 지속
       ├─ rc=2면 last_removed=Some (:426-428)
       │    └─ status()/create()/remove() 때마다 refresh_removal (:324-338)
       │         → leftcar_vdisp_is_active_v1 (bridge.m:461) 폴링으로 pending 해소
       └─ rc=1 또는 기타 → Err → current Some 유지 (회귀 테스트 failed_removal_keeps_the_current_display)
```

| # | 꺼져야 하는 시점 | destroy 호출 여부 | 근거 (파일:행) | 판정 |
|---|---|---|---|---|
| S1 | 호스트 UI 제거 버튼 | **호출됨** | `ExtendedDisplayCard.tsx:107` → `virtual_display_remove` (`lib.rs:989`) → `manager.remove` (spawn_blocking) | 정상 경로. 단 30s 지연 UX는 §5 C4 |
| S2 | 뷰어 `removeVirtualDisplay` | **호출됨** | `use-extension-display.ts:91` → `control.rs:2548` → `manager.remove` (`control.rs:2560`) | 정상 경로 |
| S3 | 리사이즈(치환) | **호출됨** (destroy→recreate) | `virtual_display.rs:450` | 정상 경로 |
| S4 | 세션 종료 (`stopStream`, LCT1 소실 포함) | **호출 안 됨** | `control.rs:3126-3160`: 세션 제거·`retire_session_capture`만 수행, vdisp 접근 전무 | **설계 의도와 버그 인지의 충돌 지점** (§5 C1/C3) |
| S5 | 시청자 연결 해제·네트워크 단절 | **호출 안 됨** | `disconnect_device_conns` (`control.rs:1060`), `disconnect_all_conns` (`control.rs:1069`) — 제어 연결만 절단 | 동일 |
| S6 | 기기 revoke/전체 revoke | **호출 안 됨** | `revoke_device` (`control.rs:956`), `teardown_targets("device_revoked")` (`control.rs:759`, `:800`) — 스트림·권한만 정리 | 동일 |
| S7 | 호스트 앱 종료 (`RunEvent::Exit`) | **호출 안 됨** | `lib.rs:375-388`: UPnP disable + `shutdown_source_access` (`control.rs:905`)만. vdisp remove 없음. `VirtualDisplayManager`에 `impl Drop` 없음(`virtual_display.rs:626`은 Default) | **구현 갭** (§5 C1) |
| S8 | SIGTERM (업데이터) | 간접 — exit(0) 후 S7과 동일 | `lib.rs:266-279` | 동일 |
| S9 | 크래시/SIGKILL | 불가(호출 자체 불가능) — WindowServer 회수 의존 | 설계 근거 design.md :56; 실측 verification.md "시험용 프로세스만 강제 종료 후 가상 화면 자동 회수" | 회수는 OS 몫 |

요약: **destroy는 S1/S2/S3뿐이다.** 세션 종료(S4), 연결 해제(S5), revoke(S6), 앱 종료(S7/S8)는
어떤 경로로도 `leftcar_vdisp_destroy_v1`에 도달하지 않는다.

---

## 4. 4a68d15가 수정한 것과 남은 것

수정된 것(확인됨):
- `init` 계열 셀렉터를 함수 포인터로 직접 호출하던 코드(`MsgInit`/`MsgInitWithDescriptor`/`MsgModeInit`)를
  ARC가 소유권 규약을 보는 선언(`NSObject (LeftcarVirtualDisplayInitializers)`, bridge.m:29-35) + 정상 ObjC 호출로 교체
  (bridge.m:314, 321-325, 343). 이전에는 레지스트리 제거 후에도 객체가 살아남아(over-retain) 제거 명령 후 화면이 남았다.
- 동일 프로세스 생성→제거 회귀는 실기 통과 (`docs/superpowers/specs/2026-09-22-sidecar-extension-verification.md`
  "실제 Mac 디스플레이 검증… 제거 후 활성 목록에 남지 않음", `Tests/VirtualDisplayBridgeProbe.swift` cycle/phantom/release-watch).

남은 것(= 잔여 결함 후보): 아래 §5.

---

## 5. 잔여 결함 후보 (4a68d15 이후 증상의 후보 목록)

### C1. 앱 종료 정리 부재 + 정적 레지스트리 + 좀비 복구 경로 없음 — 최유력
- `RunEvent::Exit`가 vdisp를 정리하지 않는다 (`lib.rs:375-388`). `shutdown_source_access`(`control.rs:905`)도
  캡처·권한만 다룬다. `VirtualDisplayManager`에는 `Drop`이 없다.
- CGVirtualDisplay의 마지막 강한 참조는 dylib의 **정적** `NSMutableDictionary`가 쥐고 있다(bridge.m:132-139, :388).
  정상 종료에서는 dealloc(=WindowServer disconnect)이 절대 실행되지 않는다.
- 설계 문서의 주장 "창 종료·앱 종료 시 정리는 객체 drop이 수행(모든 경로 보장)"(design.md :136)은 **구현과 불일치**다.
  실제로는 "프로세스 사망 시 WindowServer가 회수"(design.md :56)에 전적으로 의존한다.
- 그 회수조차 **SIGKILL 프로브로만 검증**됐다(verification.md "Mac 소유 프로세스 비정상 종료"). 정상 exit 경로에서
  회수가 늦거나 실패하면 좀비가 되는데, 복구 수단이 없다:
  - 새 프로세스의 브리지 레지스트리는 비어 있어 `destroy`는 rc=1(bridge.m:503) → Rust Err → 영구 실패.
  - Rust `current`도 None이라 `remove()`는 `Ok(true)` no-op(`virtual_display.rs:420-421`) → UI는 멀쩡해 보임.
  - `leftcar_vdisp_probe_v1`(bridge.m:226)과 `create` 중복 가드(bridge.m:297-300)는 **자기 레지스트리**만 보므로
    남의(이전 프로세스의) Leftcar 화면을 감지하지 못하고, `create`는 **두 번째** 화면을 만든다. 고정 serial
    (0x4C430001, bridge.m:123-125)로 좀비를 탐지·입양(adoption)할 수 있는데 그 경로가 구현돼 있지 않다.
- 사용자 인지와의 연결: 트레이 상주 앱이라 창을 닫아도 프로세스가 살아 있고("백그라운드 실행 중", `lib.rs:322`),
  이때는 C1이 아니라 "호스트 소유 영속" 정책 자체가 화면 유지의 원인이다(§C3).

### C2. Rust↔브리지 상태 불일치 — remove가 "성공"하지만 실제로는 아무도 안 끔
- 브리지는 성공 시에만 등록하지만(bridge.m:386-389), Rust는 등록 **이후** 결과를 파싱한다:
  - `take_json_with`가 빈 문자열을 반환하면 `bad create json` Err (`virtual_display.rs:383`) — 그러나 브리지에는
    화면이 살아 있음(등록은 브리지가 JSON 반환 전에 끝냄).
  - `displayId` 파싱 실패/0이면 Err (`virtual_display.rs:401-403`) — 동일하게 브리지 잔존.
  - 이 상태에서 Rust `current=None` → 이후 모든 `remove()`는 no-op 성공(`:420-421`). UI는 "제거됨"을 보여주지만
    화면은 프로세스가 죽을 때까지 남는다. 감사 시점 기준 확률은 낮지만 구조적으로 열려 있는 경로다.
- 역방향(레지스트리 유실, Rust current 유지)은 `call_destroy` rc=1 Err(`:578-594`)로 매 remove가 실패한다.

### C3. 정책 불일치: "꺼져야 하는 시점"의 정의가 설계와 어긋남
- 현재 설계는 세션 종료·연결 해제에 화면을 **의도적으로** 살려둔다(§1 소유 모델, design.md :74, verification.md).
- 그러나 루프 의도(버그 보고)는 세션 종료·앱 종료를 "명확히 꺼져야 하는 시점"으로 규정한다. 이것은 코드 결함이라기보다
  **정책 재결정 과제**다. 트레이드오프:
  - (a) 현행 유지 + 문서/UI 명시: 재열기 지연 없음, 좀비·혼란 리스크는 C1/C4와 별도로 남음.
  - (b) 세션 종료·마지막 시청자 해제 시 자동 destroy: 기대에 부합하나 재열기 때 생성 지연(수 초)+배치 초기화,
    제거 30s 지연 윈도우 동안 재생성 잠금(`last_removed`, `virtual_display.rs:364-366`)과 충돌 가능.
  - (c) 앱 종료 시에만 destroy (S7 훅 추가): 최소 변경으로 좀비 원천 차단. b보다 먼저 저비용이다.

### C4. 제거 지연 ~30s vs 폴링 상한 불일치 — "안 꺼진다"로 느껴지는 UX 구간
- 실측: 프로세스 생존 중 제거는 ~30s 비동기, 빈 세션 재구성 트릭도 무효(design.md :231).
- 브리지 폴링은 1.5s+8s=9.5s(bridge.m:508-516) → 실제로는 항상 rc=2 → Rust `last_removed` → UI "제거 중".
  `refresh_removal`은 status/create/remove 호출 시에만 돌고(`virtual_display.rs:295-297, 318-338`) UI 폴링은 2s
  (`ExtendedDisplayCard.tsx:133`)이라 결국 해소되지만, 최대 30s간 시스템 설정·카탈로그에 화면이 계속 보인다.
- `resize`의 `wait_for_removal` 상한은 200×100ms=20s(`virtual_display.rs:447-451, 655-666`) < 실측 30s →
  리사이즈가 "removal pending" 오류로 자주 끝날 수 있다(저장된 모드로 재시도 유도는 됨).

### C5. deactivate/비공개 해제 보조 API 미검증
- 브리지는 release-only다. CGVirtualDisplay에 deactivate 계열 비공개 메서드가 존재하는지 조사한 기록이 없고,
  30s 지연이 "deactivate 미호출" 때문일 가능성은 배제돼 있지 않다(cgvd-spark 재확인 과제).
  다만 release-only가 동일 프로세스 실기에서는 통과했으므로 우선순위는 낮다.

### C6. 카탈로그 팬텀 (관찰 필요)
- 제거 직후 WindowServer 열거가 남아 있는 동안 호스트 카탈로그(실시간 CG 열거 기반)와 뷰어 화면 목록에
  삭제된 화면이 계속 보일 수 있다. 이를 관찰하기 위한 `phantomCheck` 프로브가 존재한다
  (`Tests/VirtualDisplayBridgeProbe.swift:194-215`). 지연 구간의 일시 현상이지 영구 잔존은 아니다.

---

## 6. 우선순위 결정표

| 후보 | 증상 기여 가능성 | 수정 비용 | 순서 제안 |
|---|---|---|---|
| C1 앱 종료 정리 부재(+좀비 복구 없음) | 높음 (앱 종료·재시작 시나리오) | 중 (Exit 훅 1곳 + 좀비 탐지 1함수) | 1순위 |
| C4 폴링 상한 < 실측 지연 | 중 (지연 구간을 버그로 인지) | 낮 (상수 조정/재시도 안내) | 2순위 |
| C2 상태 불일치 no-op remove | 중 (발생 시 영구 좀비) | 낮 (create 실패 시 레지스트리 정리 호출) | 3순위 |
| C3 정책 재결정(세션 종료 연동) | 사용자 기대에 따라 높음 | 정책·UX 결정 필요 | 별도 결정 기록(ADR) 권장 |
| C5 deactivate API 조사 | 낮음 | 조사 비용 | 여유 시 |
| C6 카탈로그 팬텀 | 낮음(일시) | — | 관찰만 |

## 7. 권고 후속 검증 (회귀 검사 설계 입력)

1. `RunEvent::Exit`에서 `manager.remove()` 호출 후 활성 목록에 Leftcar 화면 0개 — 프로브 프로세스가 아닌
   실제 호스트 종료 경로로 검증(현행 검증은 SIGKILL 프로브만 존재).
2. create 실패(강제로 bad JSON/displayId=0 경로 주입) 후 remove가 브리지 잔존 객체를 회수하는지.
3. 호스트 재시작 직후 좀비 탐지: `create` 전 고정 vendor/product/serial 열거로 이전 화면 입양 또는 destroy.
4. resize 연속 3회에서 `wait_for_removal` 20s 상한 초과율 측정(실측 30s 대비).

---

## 8. 근본 원인 확정 (루프 진단 결론 — 수명주기 추적 결과 반영)

§5의 후보군에 수명주기 전수 추적(§3)과 실기 기록(§4, design.md E-1/E-5, verification.md)을 대조하여
"명확하게 꺼져야 하는 시점(세션 종료, 화면 제거 명령, 앱 종료)에 화면이 남는다"는 증상을 설명하는
근본 원인을 아래와 같이 확정한다.

### 8.1 1순위 근본 원인 — destroy 트리거 커버리지 갭 + "호스트 소유 영속" 정책

> **화면을 끄는 유일한 수단은 `leftcar_vdisp_destroy_v1`(bridge.m:502)이고, 이것에 도달하는 경로는
> 명시적 제거·리사이즈 명령 3개(S1/S2/S3)뿐이다. 버그 보고가 "꺼져야 하는 시점"으로 꼽은 나머지
> 수명주기 — 세션 종료(S4), 앱 종료(S7/S8) — 에는 destroy 호출이 코드상 존재하지 않으며, 세션 종료
> 시 화면 유지는 설계 정책이 의도한 동작이다. 증상의 본체는 ARC 결함의 재발이 아니라 "제거 트리거가
> 배치되지 않은 수명주기 + 의도적 비제거 정책"이다.**

코드 증거:

1. **세션 종료 → destroy 없음 (의도된 유지)**
   - `control.rs:3126` `"stopStream" => {` 핸들러는 세션 제거와 캡처 은퇴만 한다:
     `control.rs:3157-3160` `self.retire_session_capture(input.session, s.handle, …)`.
   - `retire_session_capture`(`control.rs:815`) → `retire_capture`(`control.rs:824`)는
     `self.backend.stop_with_reason(handle, reason)`만 호출한다. 핸들러 본문 어디에서도
     `virtual_display` 매니저를 건드리지 않는다(control.rs에서 vdisp 참조는 2567-2631의 명령 4개가 전부).
   - 이것은 버그가 아니라 정책이다. `docs/2026-09-18-extended-display-design.md:74`
     "disconnect → destroy / grace | v1은 **호스트 소유 영속**(§6 결정 1)이라 grace 타이머 자체가
     불필요", 같은 문서 :177 "실제 모니터는 뷰어가 떠난다고 뽑히지 않는다". 실기 재검증(verification.md
     태블릿 재검증 표)도 "스트림 닫기·다시 열기 | 뒤로 가기 확인으로 닫은 뒤 Mac display ID 84 유지,
     `확장 화면 열기`로 같은 화면 재사용 | 통과"로 **유지를 통상 동작으로 기록**했다.
2. **앱 종료 → destroy 없음 (OS 회수 의존)**
   - `lib.rs:375-389` `RunEvent::Exit` 처리는 `upnp.disable()`(3s 타임아웃)와
     `shutdown_source_access()`(`control.rs:905`, source_operations·펜싱·세션 캡처만)뿐이고
     `VirtualDisplayManager`를 호출하지 않는다. SIGTERM도 `lib.rs:273-279`에서
     `handle.exit(0)`으로 Exit 이벤트로 우회되므로 같은 갭이다.
   - `VirtualDisplayManager`에는 `impl Drop`이 없다(rg 검색 결과 없음). 브리지의 마지막 강한 참조는
     dylib 정적 레지스트리가 쥐고 있으므로(bridge.m:132 `static NSMutableDictionary *VDRegistry`,
     등록 bridge.m:388) 정상 종료 경로에서 dealloc(=WindowServer disconnect)은 실행되지 않는다.
   - 다만 프로세스 사망 자체의 회수는 실물 검증이 두 번 있다: design.md:228 E-1 "소유 프로세스 kill -9
     시 즉시 회수. 실운영에서도 재현: 호스트 재시작 시 가상 화면 소실(설계대로)". 즉 앱 종료 잔존은
     "프로세스가 죽어도 화면이 남는다"가 아니라 "트레이 상주로 프로세스가 살아 있는 동안(그리고 종료
     처리 중) 정책상 화면이 유지된다"로 해석해야 한다.
3. **화면 제거 명령 → destroy 도달, 그러나 지연 구간이 "안 꺼짐"으로 인지됨**
   - `virtual_display.rs:417-431` `remove_inner`가 `call_destroy`(`virtual_display.rs:578-594`)로
     브리지 destroy를 부르고, 4a68d15 이후 실기에서 제거 소실이 확인됐다(verification.md
     "제거 뒤 기본 화면 ID 3, `0,0 1920x1080`만 남았다").
   - 잔여 기전은 지연이다: 실측 제거는 ~30초 비동기(design.md:231 E-5, "빈 세션 재구성 트릭도 무효"),
     브리지 폴링은 1.5s+8s=9.5s(bridge.m:510, bridge.m:516)라 항상 rc=2로 끝나고
     `last_removed`(virtual_display.rs:426-428) → UI "제거 중"이 최대 30초 지속된다.
     최종 소실은 보장되므로(실기 확인) 이것은 부가 기전이지 근본 원인이 아니다.

### 8.2 4a68d15와의 관계

- 4a68d15가 고친 것은 "destroy가 **호출되는** 경로 안에서"의 over-retain이다(init family를 ARC가
  소유권 규약을 보게 선언하고 정상 ObjC 호출로 교체, bridge.m:29-35). 그 효과는 동일 프로세스
  생성→제거 실기로 검증됐다(§4). 남은 증상은 이 수정 범위 밖이다:
  - (미흡했던 부분이 아니라) **원래 수정 대상이 아니었던 영역** — 세션 종료(S4)·앱 종료(S7/S8)에는
    애초에 destroy 호출이 없었고, 호스트 소유 영속 정책(design.md:74, :177)은 4a68d15 전후가 같다.
  - **새로 생긴 부작용 없음** — 4a68d15 이후 실기 제거가 통과하고(verification.md), 본 감사의 코드
    재검(§3 표, §8.1)에서도 이 수정이 유지되어야 할 객체를 만들거나 제거를 막는 경로는 발견되지 않았다.
- 기록 가치: 4a68d15 커밋 메시지의 "제거 후에도 화면이 남는 문제를 해결한다"는 사용자가 인지한 증상
  전체를 지칭했으나 실제 수정은 그중 제거 명령 경로의 해제 확실성이었다. 증상이 "남아 있다"고 느껴진
  것은 수정 실패가 아니라 미커버 영역(정책+트리거 갭, 제거 지연 구간) 때문이다.

### 8.3 대안 가설과 기각 이유

| 가설 | 기각 사유 |
|---|---|
| C1 좀비(사망 후 잔존 + 입양 갭)를 1순위로 | 프로세스 사망 회수는 E-1로 두 번 실물 확인(kill -9 + 실제 호스트 재시작 소실). 앱 종료 잔존은 "프로세스가 안 죽은" 상태의 정책 문제로 설명되므로 좀비 가설은 증상의 주 설명이 못 된다. 단, 회수 지연 창과 무입양 구조(bridge.m:297-300 중복 가드가 자기 레지스트리만 봄)는 방어 코드 부재로 잔여 리스크로 유지한다. |
| C2 Rust↔브리지 상태 불일치 no-op remove를 1순위로 | 구조적으로는 열려 있으나(virtual_display.rs:383/:401 파싱 실패 시 current=None, bridge.m:388은 등록 후 JSON 반환) 관측·재현 증거가 없고 4a68d15 후 실기 제거가 통과했다. 발생 시 영구 좀비가 되는 잠재 결함으로 별도 추적한다. |
| C4 30초 지연을 근본 원인으로 | 최대 30초의 **일시** 잔존이고 최종 소실이 실기로 확인됐다. "안 꺼진다"는 인지를 증폭시키는 부가 기전으로 §8.1-3에 반영. C5(deactivate 부재)가 이 30초의 내부 원인일 가능성은 의존 태스크가 미확정으로 남겼으므로(cgvd-spark 재확인 필요) 가설로만 기록한다. |
| C6 카탈로그 팬텀 | 지연 구간의 일시 열거 잔존. 영구 잔존을 만들지 못해 기각(관찰 전용). |

### 8.4 확정 결론이 남기는 잔여 리스크 (수정 과제의 입력)

1. **정책 재결정(필수 선행)**: 버그 보고는 세션 종료·앱 종료를 "꺼져야 하는 시점"으로 규정한다. 이는
   design.md:74/:177의 호스트 소유 영속 정책과 정면충돌이므로, 코드 수정 전에 ADR로 정책을 재결정해야
   한다(§5 C3의 트레이드오프 (a)/(b)/(c) 참조). 정책이 유지된다면 세션 종료 시의 잔존은 정상 동작이다.
2. **앱 종료 훅(S7)**: 최소 변경(정책 (c)) — `RunEvent::Exit`에서 `manager.remove()` 호출. 다만 브리지
   destroy가 최대 9.5s 폴링을 도므로 종료 지연 상한을 정해야 하고, rc=2여도 프로세스 사망 후 WindowServer가
   회수하므로(E-1) "호출만 보장"으로 충분하다.
3. **좀비 입양/복구 부재**: 고정 serial(0x4C430001)로 이전 프로세스의 잔존 화면을 탐지·destroy하는
   경로가 없다(bridge.m:297-300 중복 가드는 자기 레지스트리만 봄). 현재 관측된 회수 실패는 없지만,
   C2와 합쳐지면 사용자가 스스로 지울 수 없는 화면이 된다.
4. **제거 지연 30초(C4)와 폴링 상한 불일치**: rc=2 규환 구조는 유지되더라도 UI 노출("제거 중" 30s)과
   resize의 20s 상한(virtual_display.rs:447-451)은 실측과 어긋난다. C5 확인(cgvd-spark) 후 함께 다룬다.

---

## 9. 수정 방침 (결정 기록 — 구현 태스크 입력)

> 결정일: 2026-09-22 · 결정자: 수정 방침 결정 태스크(decision-fix-approach) · 입력: §8 확정 결론 +
> 진단 태스크 잔여 리스크 ①②③. 본 섹션은 §8.4-1이 요구한 **정책 재결정(ADR 성격)** 을 포함한다.

### 9.1 수정 계층 결정 — **host Rust 단일 계층 (브리지 무변경)**

수정 계층 후보는 Swift `CGVirtualDisplayBridge` / host Rust(`virtual_display`·`control`·`lib`)/양쪽이었다.
**host Rust 단일 계층**으로 결정한다. 근거:

1. 확정된 근본 원인(§8.1)은 destroy 해제 로직의 결함이 아니라 **destroy 트리거가 배치되지 않은 수명주기 +
   소유 정책**이다. 트리거를 배치하는 곳은 제어 흐름의 소유자, 즉 상태(`current`, `last_removed`)와 정책을
   보유한 Rust 매니저·컨트롤 서버다.
2. 브리지의 "release-only + 프로세스 사망 시 WindowServer 회수" 모델은 4a68d15가 수정·실기 검증한
   영역이다(§4). 검증된 해제 경로를 정책 변경 목적으로 재개편하면 회귀 위험만 추가된다.
3. 계층 경계: 정책(언제 꺼야 하나)을 브리지에 넣으면(예: atexit 스윕, 자동 입양) 호스트 UI·뷰어 상태와
   브리지 실제 상태가 갈라진다 — 호스트가 모르는 사이 화면이 사라져 `status()`·카탈로그와 불일치한다.
   브리지는 기계적 해제만 담당하고, 수명 결정은 전부 호스트가 내린다.

### 9.2 정책 재결정 — 호스트 소유 영속 → **세션 스코프 소유 + 종료 시 해제**

design.md §6 결정 1의 "호스트 소유 영속"을 이 루프에서 재결정한다(design.md가 예시해 둔 "세션 소유+grace를
원하면 별도 논의"의 그 논의). 버그 보고가 세션 종료·앱 종료를 "명확히 꺼져야 하는 시점"으로 규정했으므로:

- **새 규칙: 라이브 뷰어 세션이 0이 되는 전환 시점에 destroy한다.** 명시적 생성(호스트 UI "만들기",
  뷰어 `createVirtualDisplay`)은 여전히 호스트가 소유하며, 세션 teardown **전환**에만 접힌다.
- 구현은 상태 재조정(주기 reconcile)이 아니라 **전환 기반 훅**이다. reconcile로 만들면 세션 없이 호스트 UI가
  만든 디스플레이가 즉시 죽는 사고가 생긴다(G1은 뷰어 세션 없이 존재할 수 있음). 전환 훅은 이를 피한다.
- 비자발적 단절(네트워크 단절·뷰어 재시작)도 규칙에 포함한다. 비용은 재열기 시 최대 ~30s의
  `removal pending` 윈도우인데, 기존 UX가 이미 처리한다 — 뷰어는 `extensionRemovalPending`일 때 open을
  가드하고(use-extension-display.ts:77) status를 3s 폴링하므로(同 :34) 프런트 변경이 불요하다.
  grace 타이머를 다시 도입하는 것보다 규칙 하나가 단순하다(design.md가 grace를 기각한 방향과 같다).

### 9.3 구체 수정 접근 (구현 태스크용 파일 단위 변경 목록)

| # | 파일 | 변경 |
|---|---|---|
| 1 | `apps/host-desktop/src-tauri/src/control.rs` | `release_virtual_display_if_idle(&self, reason: &str)` 헬퍼 추가: `virtual_display.get()`이 Some이고 teardown 후 `sessions.lock().live.is_empty()`면 `tauri::async_runtime::spawn_blocking`으로 `manager.remove()`를 실행(응답을 막지 않음 — 브리지 폴링 최대 9.5s는 백그라운드), 실패는 `eprintln!`만. 감사 로그 `virtual_display_released`(`reason` 포함). 호출 지점 4곳: ① `stopStream` 성공 분기(≈:3160, `Ok(result)` 이후) ② `teardown_targets` 말미(≈:788) ③ 만료 세션 스윕의 `removed` 후(≈:1877) ④ `stop_sessions_for_viewer` 루프 후(≈:2239). `force_stop_session`·`shutdown_source_access`는 세션을 `live`에서 바로 빼지 않거나 종료 경로라 호출하지 않는다(각각 스윕과 lib 훅이 커버). |
| 2 | `apps/host-desktop/src-tauri/src/lib.rs` | `RunEvent::Exit`(≈:375)에서 `shutdown_source_access` 뒤에 vdisp 해제 추가: `app.state::<Arc<VirtualDisplayManager>>()`를 얻어 `std::thread::spawn`으로 `remove()` 실행, `mpsc::channel` + `recv_timeout(2s)`으로 상한 2초만 대기 후 종료 진행. 오류는 기존 exit 훅 스타일(`eprintln!`)로만 기록. 잔여 정리는 프로세스 사망 시 WindowServer 회수(E-1 실검, design.md:228)가 백스톱. SIGTERM은 `handle.exit(0)`→Exit 경로로 자동 커버(lib.rs:273-279). tokio 런타임이 꺼가는 시점이라 `async_runtime` 대신 `std::thread`를 쓴다. |
| 3 | `apps/host-desktop/src-tauri/src/virtual_display.rs` | resize 제거 대기 상한을 상수화: `REMOVAL_WAIT_ATTEMPTS: usize = 350`(=35s, 실측 ~30s + 여유)로 `wait_for_removal(..., 200)` 호출(≈:451) 대체, 근거 주석(design.md E-5 ~30s) 부착. 제거 명령 경로 자체(S1/S2/S3)는 §4 실기 통과로 **무수정**. |
| 4 | `docs/2026-09-18-extended-display-design.md` | §6 결정 1과 §2 표 "disconnect → destroy / grace" 행, :177 "실제 모니터는…" 문장을 9.2의 재결정으로 갱신(정책 문서-코드 정합 유지). |

안전성 근거: 훅의 `remove()`와 `resize()`의 `remove_inner()`는 매니저의 `operation` 뮤텍스로 직렬화되므로
뷰어 리사이즈 흐름(stopStream으로 스트림을 끊고 resizeVirtualDisplay)에서 훅·리사이즈가 이중 destroy해도
안전하며, resize의 `wait_for_removal`이 훅이 남긴 `last_removed`를 그대로 기다린다. 다른 기기의 세션이
남아 있으면 `live.is_empty()` 게이트가 발동을 막아 A 기기 revoke가 B 기기 시청을 깨지 않는다.

### 9.4 회귀 검사 (구현 태스크가 추가할 테스트)

1. **cargo test** (`apps/host-desktop/src-tauri`):
   - control.rs: ① 세션 전부 제거 시 훅이 `remove`를 호출하는지(실 매니저 주입 — macOS lib 부재 환경에서
     remove는 no-op/Err로 끝나며 훅이 이를 명령 응답 오류로 전파하지 않아야 함) ② 다른 기기 세션 잔존 시
     호출하지 않는 게이트 ③ `stopStream` 이후에도 세션이 남으면 미발동.
   - virtual_display.rs: `REMOVAL_WAIT_ATTEMPTS × 100ms ≥ 30s(실측)` 상수 자기검사 테스트 추가. 기존
     `failed_removal_keeps_the_current_display`, `resize_waits_through_delayed_removal` 회귀 유지 확인.
2. **shim 테스트 빌드**: 브리지 무변경이지만 `tools/build-n.zsh` 라이브러리·테스트 모드 빌드가 깨지지 않는지 확인(verify.mjs 경로).
3. **vitest/typecheck**: 프런트 무변경이지만 루프 게이트로 실행. TSX/React 변경이 없으므로 react-doctor 게이트는 이 태스크 범위에서 미해당(프런트를 건드릴 경우에만).
4. **실기 검증 권고(검증 스펙 문서에 추가)**: 뷰어 뒤로 가기 닫기 → Mac Displays에서 소실 확인 / 태블릿 앱 강제 종료 → 미디어 타임아웃 후 소실 확인 / 뷰어 재시작 → 재열기가 pending 해소 후 재생성되는지(지연 체감 측정 포함) / 호스트 종료(Quiet 메뉴) → 소실 확인.

### 9.5 기각된 대안

| 대안 | 기각 사유 |
|---|---|
| 브리지 계층 수정(atexit 해제 스윕, 폴링 단축, 좀비 입양) | 9.1 근거 2·3. 정책을 브리지가 갖면 호스트 상태와 불일치하고, 4a68d15가 검증한 release-only 모델의 회귀 위험. 좀비 입양은 관측 결함 없음(§8.3) + 새 심볼·CG 열거라 검증 비용 대비 효과 낮음 — **후속 과제로 이월**(§8.4-3). |
| 정책 (a) 유지(세션 종료 시 유지, 문서·UI로 정리) | 루프 의도가 세션 종료를 꺼짐 시점으로 명시하므로 증상이 남는다. 기각.
| grace 타이머 방식(비자발적 단절만 유예 후 destroy) | design.md가 v1에서 기각한 타이머 재도입. 규칙 하나("세션 0 전환 시 destroy")로 통일하는 편이 단순하고, 재열기 지연은 기존 removalPending UX가 흡수(9.2). 기각하되, 재시작 재열기 지연이 실기에서 체감되면 재검토 대상으로 기록.
| `createVirtualDisplay`가 pending을 서버에서 대기·재시도 | control 명령이 최대 ~35s 붙잡혀 타임아웃·응답 없음으로 오인될 위험. 뷰어의 pending 가드+폴링으로 충분. 기각.
| C2(create 파싱 실패 시 브리지 잔존 회수) 동반 수정 | Err 이후 경로에서는 displayId를 알 수 없어(virtual_display.rs:383, :401) Rust-only 회수가 불가능하고, 브리지 스윕 심볼이 필요해 9.1 결정과 충돌. 관측 결함 없음 → 후속 과제 이월(§8.4-3과 통합). |

### 9.6 수용된 잔여 리스크

1. **재열기 지연 윈도우**: 세션 종료 직후 ~30s 내 재열기 시 pending으로 open이 무동작(use-extension-display.ts:77 가드). deactivate 조사에서 단축용 셀렉터가 확인되지 않았으므로 이 지연은 남는다.
2. **해소 — C2·좀비 입양**: 브리지의 고정 hardware identity 기반 stale 탐지와 Rust의 생성 전·생성 결과 파싱 실패·`displayId=0` 복구 호출로 방어 경로를 추가했다(§10.1). 회수 완료 전에는 `last_removed`로 pending을 유지한다.
3. **teardown↔startStream 경합**: 마지막 세션 teardown 직후 새 스트림 시작이 끼어들면 새 스트림의 소스가 제거 중일 수 있다(확률 낮음, 재시도로 회복). v1 수용.
4. **종료 시 2초 상한 대기**: exit가 최대 2초 늦어진다. rc=2(해제됐으나 열거 잔존)여도 사망 회수가 보장하므로 "호출 보장"으로 충분(§8.4-2 채택).

---

## 10. 후속 조치 결과

### 10.1 좀비 복구와 C2 방어

- 브리지에 `leftcar_vdisp_find_stale_v1`(`CGVirtualDisplayBridge.m:259`)을 추가했다. 현재 브리지 레지스트리 소유분은 제외하고, Leftcar의 고정 vendor/product/serial과 일치하는 온라인 디스플레이를 찾아 이전 프로세스 잔존분을 반환한다.
- `VirtualDisplayManager::recover_stale`은 `create` 진입 시와 create 결과 JSON 파싱 실패 또는 `displayId=0` 판정 시 호출된다. 발견한 디스플레이를 destroy하고, 즉시 소실하지 않거나 회수 호출이 실패하면 `last_removed`에 `stale:<displayId>`를 기록해 새 identity 생성을 pending 상태로 막는다. 즉시 회수 성공은 감사 이벤트 `virtual_display_zombie_recovered`로 남긴다.
- 이로써 §9.6-2의 이전 프로세스 좀비 입양 부재와 C2의 생성 결과 불일치 방어 항목을 해소 처리했다.

### 10.2 deactivate 조사 결론

`docs/research/2026-09-24-cgvd-deactivate-investigation.md`의 확정 결론은 다음과 같다.

> **미확인 — 조사한 공개 헤더·사용 코드와 macOS 26.6.2 런타임 어디에서도 `CGVirtualDisplay`의 deactivate/disable/invalidate/stop/terminate/disconnect 계열 셀렉터는 확인되지 않았다. 따라서 현재 근거로는 release-only 대비 30초 제거 지연을 줄일 호출이 없으며, C5의 “deactivate 미호출” 가설은 기각한다.**

따라서 확인되지 않은 비공개 셀렉터를 추측해 호출하지 않고 release-only 모델을 유지한다. WindowServer 내부의 약 30초 지연 원인은 여전히 미해결이다.

### 10.3 정책 문서 정합

`docs/2026-09-18-extended-display-design.md`를 **세션 스코프 소유** 정책으로 갱신했다. 라이브 뷰어 세션이 0이 되는 teardown 전환에서 destroy하고, 앱 종료 시 최대 2초 동안 해제 호출 완료를 기다리며, 별도 grace 타이머는 두지 않는다. 이로써 §9.3-4의 설계 문서 갱신 항목은 해소됐다.

### 10.4 최종 검증 게이트

후속 구현이 반영된 최종 작업 트리에서 다음 게이트를 다시 실행했고 모두 종료 코드 0으로 통과했다.

- `bun run typecheck` — 통과.
- `bun run test` — 통과(70개 파일, 852개 테스트).
- `bun run test:architecture` — 통과(`architecture-check: TS/Kotlin rules clean`).
- `cd apps/host-desktop/src-tauri && cargo test` — 통과(단위 테스트 290개, E2E 테스트 13개, 실패 0개).

검증 시 React/JSX/TSX 변경은 없어 React Doctor는 적용 대상이 아니었다.

### 10.5 사전 존재 위반과 검증 도구 기록

- f3e71c9에서 유입된 `StreamPointerDiagnostics.kt`의 `org.json.JSONObject` import가 Kotlin import allowlist를 위반해 아키텍처 게이트를 막고 있었다. allowlist를 늘리지 않고 해당 의존을 제거해 숫자 필드 전용 수동 JSON 직렬화로 바꿨으며, §10.4의 아키텍처 게이트 통과로 복구를 확인했다.
- §9.4-2가 지목한 `tools/build-n.zsh`는 저장소에 존재하지 않아 그 경로를 통한 별도 shim 빌드 검증은 실행할 수 없었다. 이 사실은 cargo 및 전체 회귀 게이트 통과와 별개이며, 해당 스크립트 기반 검증을 통과한 것으로 간주하지 않는다.

### 10.6 남은 리스크와 실기 권고

해소되지 않은 리스크는 §9.6-1의 **재열기 지연 윈도우**, §9.6-3의 **teardown↔startStream 경합**, §9.6-4의 **종료 시 최대 2초 대기**다. 또한 §9.4-4의 실기 검증은 여전히 권고한다: 뷰어 뒤로 가기와 앱 강제 종료 뒤 Mac Displays 소실, pending 해소 뒤 재열기, 호스트 Quiet 종료 뒤 소실을 실제 기기에서 확인해야 한다.
