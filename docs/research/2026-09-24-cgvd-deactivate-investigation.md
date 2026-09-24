# CGVirtualDisplay deactivate 계열 비공개 API 조사 — 2026-09-24

목적: 가상 디스플레이 제거가 약 30초 걸리는 현상이 `CGVirtualDisplay`의 deactivate/비활성화 API를 호출하지 않아서 생기는지 확인하고, 감사 C5의 후속 조치 근거를 남긴다. 코드 변경은 하지 않는다.

조사 범위: 공개 역공학 원본(KhaosT/CGVirtualDisplay), 공개 사용 사례(OpenDisplay), 이 Mac의 Objective-C 런타임, 저장소의 `docs/**`, `native/macos-capture-shim/Sources/VirtualDisplay/**`, `vendor/**`.

## 1. 확정 결론

> **미확인 — 조사한 공개 헤더·사용 코드와 macOS 26.6.2 런타임 어디에서도 `CGVirtualDisplay`의 deactivate/disable/invalidate/stop/terminate/disconnect 계열 셀렉터는 확인되지 않았다. 따라서 현재 근거로는 release-only 대비 30초 제거 지연을 줄일 호출이 없으며, C5의 “deactivate 미호출” 가설은 기각한다.**

여기서 “미확인”은 모든 과거·미래 macOS 바이너리에 그런 진입점이 없다는 보장이 아니라, **지원 가능한 셀렉터와 시그니처를 확보하지 못했다**는 판정이다. 비공개 API인 만큼 다른 OS 빌드의 내부 C 함수나 이름이 다른 메서드까지 부재라고 일반화하지 않는다.

## 2. 조사 환경과 방법

- 실측 환경: macOS 26.6.2 (25G83), arm64, Xcode 26.2 (17C52).
- 네트워크 접근은 가능했다. 공개 저장소를 2026-09-24에 읽고 커밋을 고정했다.
  - KhaosT/CGVirtualDisplay `ec72be5f546d1aa1257f9976fea86a2de9dab35c`
  - peetzweg/opendisplay `2e74ced51924bbda14a4d897a8459b710b37a7d9`
- 로컬 Objective-C 프로브는 `NSClassFromString`으로 클래스를 로드한 뒤 `class_copyMethodList`로 `CGVirtualDisplay`, `CGVirtualDisplayDescriptor`, `CGVirtualDisplaySettings`, `CGVirtualDisplayMode`의 **직접 인스턴스 메서드 전체**와 `method_getTypeEncoding`을 출력했다. 이어 `class_getInstanceMethod`로 후보 셀렉터 12개를 개별 조회했다.
- `vendor/**`에는 `CGVirtualDisplay`, `virtual display`, deactivate 계열 관련 참고 자료가 없었다(`find ... | grep -IEni`, 일치 0건).
- 저장소의 cgvd-spark 결과는 클래스 4종과 메서드 수를 런타임에서 조사한 선행 실측이다(`docs/research/2026-09-03_cgvd-spark-results.md:25-27`, `:49-53`). 이번에는 그 방법을 제거 셀렉터와 타입 인코딩까지 좁혀 재검했다.

## 3. 근거

### 3.1 공개 역공학 원본: 생성·설정만 있고 명시적 비활성화는 없다

Khaos Tian의 최초 공개 역공학 헤더는 `CGVirtualDisplay`에 속성 getter들과 다음 두 동작만 선언한다.

- `-initWithDescriptor:`
- `-applySettings:`

근거: [`VirtualDisplayExp/CGVirtualDisplayPrivate.h:34-52`](https://github.com/KhaosT/CGVirtualDisplay/blob/ec72be5f546d1aa1257f9976fea86a2de9dab35c/VirtualDisplayExp/CGVirtualDisplayPrivate.h#L34-L52). deactivate/destroy/disable/invalidate 계열 선언은 없다. 샘플은 `CGVirtualDisplay?`를 강한 속성으로 보유하고(`ViewController.swift:13`), 생성한 객체를 그 속성에 넣은 뒤 설정을 적용한다(`:32-42`). 별도 종료 메서드는 호출하지 않는다: [`ViewController.swift:13-42`](https://github.com/KhaosT/CGVirtualDisplay/blob/ec72be5f546d1aa1257f9976fea86a2de9dab35c/VirtualDisplayExp/ViewController.swift#L13-L42).

이 자료는 Apache-2.0 원본 공개 프로젝트이며, 헤더 생성일은 2021-02-17이다(`CGVirtualDisplayPrivate.h:5`). 오래된 자료 하나만으로 최신 런타임을 단정하지 않기 위해 §3.2와 §3.3을 함께 확인했다.

### 3.2 최신 공개 사용 사례: 여전히 release가 제거 계약이다

OpenDisplay의 2026-09-23 커밋도 역공학 인터페이스에서 `CGVirtualDisplay` 동작을 `initWithDescriptor:`와 `applySettings:`만 선언한다: [`Mac/CGVirtualDisplayPrivate.h:36-54`](https://github.com/peetzweg/opendisplay/blob/2e74ced51924bbda14a4d897a8459b710b37a7d9/Mac/CGVirtualDisplayPrivate.h#L36-L54). 즉 2021 원본 이후의 실사용 헤더에도 deactivate 계열 셀렉터나 시그니처가 추가되지 않았다.

OpenDisplay 구현은 수명 루프가 `CGVirtualDisplay`를 잠깐 강하게 잡지 않도록 주의하면서, **release가 디스플레이 제거를 일으킨다**고 명시한다(`VirtualDisplay.swift:91-92`). 회전 시 release/recreate 대신 `applySettings:`를 쓰는 이유도 release가 WindowServer의 창 재배치를 일으키기 때문이라고 설명한다(`:104-108`): [`Mac/VirtualDisplay.swift:88-108`](https://github.com/peetzweg/opendisplay/blob/2e74ced51924bbda14a4d897a8459b710b37a7d9/Mac/VirtualDisplay.swift#L88-L108).

`terminationHandler`는 “시스템이 디스플레이를 종료했을 때” 받는 descriptor 콜백으로 사용된다(`VirtualDisplay.swift:64-66`). 호출자가 종료를 요청하는 메서드가 아니다: [`Mac/VirtualDisplay.swift:53-68`](https://github.com/peetzweg/opendisplay/blob/2e74ced51924bbda14a4d897a8459b710b37a7d9/Mac/VirtualDisplay.swift#L53-L68).

### 3.3 macOS 26.6.2 런타임: 후보 셀렉터 모두 부재

`class_copyMethodList(CGVirtualDisplay, ...)` 결과는 직접 메서드 21개였다. 동작 메서드는 다음 셋뿐이고, 나머지는 getter와 `dealloc`이다.

| 런타임 셀렉터 | Objective-C type encoding | 해석 |
|---|---|---|
| `initWithDescriptor:` | `@24@0:8@16` | `id (id self, SEL, id descriptor)` |
| `applySettings:` | `B24@0:8@16` | `BOOL (id self, SEL, id settings)` |
| `dealloc` | `v16@0:8` | ARC/런타임 해제 |

나머지 직접 셀렉터는 `productID`, `vendorID`, `displayID`, `rotation`, `name`, `terminationHandler`, `queue`, `serialNumber`, `sizeInMillimeters`, 색좌표, `hiDPI`, 최대 픽셀, `modes`, `serialNum` getter다. 이는 선행 cgvd-spark가 같은 OS에서 기록한 `CGVirtualDisplay` 21개 메서드와도 일치한다(`docs/research/2026-09-03_cgvd-spark-results.md:25-27`).

`class_getInstanceMethod`로 상속 메서드까지 포함해 아래 후보를 조회한 결과는 모두 `ABSENT`였다.

```text
deactivate       ABSENT    deactivate:       ABSENT
invalidate       ABSENT    invalidate:       ABSENT
disable          ABSENT    disable:          ABSENT
stop             ABSENT    stop:             ABSENT
terminate        ABSENT    terminate:         ABSENT
disconnect       ABSENT    disconnect:        ABSENT
```

따라서 대상 OS에서 호출 가능한 deactivate 계열 **셀렉터도, 기록할 시그니처도 없다**. `dealloc`은 직접 호출할 API가 아니며 ARC가 마지막 강한 참조 해제 시 수행하는 수명주기 메서드다.

### 3.4 Leftcar 구현과 실측은 release-only 모델과 일치한다

Leftcar 브리지는 레지스트리가 `CGVirtualDisplay` 강한 참조를 보유한다(`native/macos-capture-shim/Sources/VirtualDisplay/CGVirtualDisplayBridge.m:120-139`, 등록 `:386-389`). 제거는 레지스트리에서 객체와 모드 레코드를 빼는 ARC release이며(`:502-507`), 이후 활성 목록을 폴링하고 빈 세션 재구성을 한 번 시도한다(`:508-518`). deactivate 셀렉터 호출은 없다.

이 release-only 경로는 반복 생성/제거에서 최종 소실과 좀비 없음이 실측됐지만, 프로세스 생존 중 소실은 약 30초 걸렸다(`docs/2026-09-18-extended-display-design.md:228-231`). 감사는 이 차이를 C5로 열어 두었으나(`docs/research/2026-09-22-virtual-display-shutdown-audit.md:146-149`), 이번 조사 결과 호출 가능한 보조 셀렉터의 근거는 발견되지 않았다.

## 4. 30초 지연에 대한 판정

### 현재 예상 효과

- **release-only 대비 단축 효과: 기대할 근거 없음.** 존재와 시그니처가 확인된 deactivate API가 없으므로 비교 호출을 구현하거나 A/B 측정할 수 없다.
- 따라서 약 30초는 “알려진 deactivate 호출 누락”으로 설명할 수 없다. 현재 증거가 지지하는 모델은 ARC release 뒤 WindowServer가 비동기로 활성 목록과 데스크톱 토폴로지를 정리한다는 것이다. 브리지의 1.5초 + 8초 폴링/빈 구성 sweep 뒤에도 남고(`CGVirtualDisplayBridge.m:508-518`), E-5에서 최종 소실은 확인됐다(`docs/2026-09-18-extended-display-design.md:231`).
- 이 조사는 WindowServer 내부의 30초 타이머 원인까지 밝힌 것은 아니다. 비공개 서버 구현과 로그에 대한 1차 근거가 없으므로 “왜 정확히 약 30초인가”는 미해결이다.

### 후속 실험의 재개 조건

다음 중 하나가 생길 때만 C5를 다시 연다.

1. 대상 macOS 런타임에서 새 deactivate 계열 셀렉터와 정확한 type encoding이 관측됨.
2. Apple 바이너리/공개 역공학 자료에서 호출 순서와 시그니처가 교차 확인됨.
3. 새 OS에서 release-only 제거가 최종 소실하지 않는 회귀가 실측됨.

그때에도 “호출 성공”이 아니라 동일 identity로 최소 5회 생성→제거한 A/B에서 활성 목록 소실 시간, 창 재배치, 미러/배치 상태, 크래시 여부를 비교해야 한다.

## 5. 계층 경계와 적용 리스크 (§9.1)

감사 §9.1은 **수명 정책은 host Rust**, 브리지는 기계적 해제만 담당하도록 결정했다(`docs/research/2026-09-22-virtual-display-shutdown-audit.md:270-282`). 이 경계에서 선택지는 다음과 같다.

| 대안 | 계층 정합 | 효과/리스크 | 판정 |
|---|---|---|---|
| A. 현행 release-only 유지 | 정합. Rust가 제거 시점을 결정하고 브리지는 레지스트리 참조만 해제 | 반복 제거 실측 통과. 약 30초 pending은 남음 | **채택** |
| B. 확인되지 않은 selector 이름을 추측해 호출 | 표면상 브리지의 기계 동작이지만, 정책 계층보다 먼저 **ABI 안전성**을 위반 | unrecognized selector, 잘못된 calling convention, WindowServer 상태 손상, OS 버전별 크래시 가능. 단축 효과 근거 없음 | 기각 |
| C. 향후 확인된 deactivate를 Rust의 destroy 요청 안에서만 호출 | Rust가 여전히 “언제”를 결정하므로 원칙적으로 §9.1 준수 가능 | runtime probe, OS별 feature gate, release fallback, 실기 A/B가 필요. bridge 자동 teardown/atexit/reconcile로 확대하면 §9.1 위반 | 조건부 후속 |

특히 브리지 자체가 세션 수나 앱 정책을 보고 자동 deactivate하면 감사가 금지한 상태 분리가 재발한다. 향후 API가 발견되어도 `leftcar_vdisp_destroy_v1`의 명시적 요청 내부에 한정하고, 실패 시 현재 ARC release로 되돌아가야 한다.

## 6. 근거 목록

| 근거 | 관찰 |
|---|---|
| KhaosT/CGVirtualDisplay 헤더 [`CGVirtualDisplayPrivate.h:34-52`](https://github.com/KhaosT/CGVirtualDisplay/blob/ec72be5f546d1aa1257f9976fea86a2de9dab35c/VirtualDisplayExp/CGVirtualDisplayPrivate.h#L34-L52) | 원 공개 인터페이스는 init/apply만 선언 |
| KhaosT 샘플 [`ViewController.swift:13-42`](https://github.com/KhaosT/CGVirtualDisplay/blob/ec72be5f546d1aa1257f9976fea86a2de9dab35c/VirtualDisplayExp/ViewController.swift#L13-L42) | 객체 강한 보유 + apply, 종료 호출 없음 |
| OpenDisplay 헤더 [`CGVirtualDisplayPrivate.h:36-54`](https://github.com/peetzweg/opendisplay/blob/2e74ced51924bbda14a4d897a8459b710b37a7d9/Mac/CGVirtualDisplayPrivate.h#L36-L54) | 최신 공개 사용 사례도 init/apply만 선언 |
| OpenDisplay 구현 [`VirtualDisplay.swift:88-108`](https://github.com/peetzweg/opendisplay/blob/2e74ced51924bbda14a4d897a8459b710b37a7d9/Mac/VirtualDisplay.swift#L88-L108) | release가 제거·WindowServer 창 재배치를 유발한다고 명시 |
| macOS 26.6.2 Objective-C 런타임 프로브 | 직접 메서드 21개, deactivate 계열 후보 12개 전부 `ABSENT`; 확인된 동작 시그니처는 init/apply/dealloc뿐 |
| `docs/research/2026-09-03_cgvd-spark-results.md:25-27`, `:49-53` | 같은 OS의 클래스 생존 및 `CGVirtualDisplay` 21메서드 선행 실측 |
| `native/macos-capture-shim/Sources/VirtualDisplay/CGVirtualDisplayBridge.m:502-518` | Leftcar는 레지스트리 release 후 활성 목록 폴링/sweep 수행 |
| `docs/2026-09-18-extended-display-design.md:228-231` | kill 회수 및 반복 제거 최종 성공, 프로세스 생존 제거 약 30초 실측 |
| `vendor/**` 전수 문자열 검색 | 관련 참고 자료 0건 |

## 7. 남은 불확실성

- Apple이 공개하지 않은 WindowServer 내부 지연 원인은 확인하지 못했다.
- 이번 런타임 결과는 macOS 26.6.2 빌드 25G83에 한정된다. 다른 빌드에서 메서드 표면이 바뀔 수 있다.
- “deactivate”가 Objective-C 셀렉터가 아닌 비공개 C 함수/XPC 프로토콜로 존재할 가능성은 이번 질문의 공개 자료와 로컬 클래스 표면에서 확인되지 않았다. 근거 없이 탐색·호출하는 것은 제품 코드 후속 조치로 권고하지 않는다.
