# 기존 Mac–Android 입력 공유 구현 비교 — 2026-10-04

기존 구현은 있다. **Mac에 연결된 마우스·키보드를 Android로 보내고 되돌아오는 방식은 구현되어 있다.** 다만 아래 소스는 Android의 실제 시스템 커서를 관찰해 반환하는 모델이 아니라, Mac/Synergy에서 관리하는 가상 좌표·이동량으로 반환을 판단한다. 그래서 현재 Leftcar의 제한된 접근성 관찰 API를 반드시 필요로 하는 것은 아니다. 이것이 Android에 연결된 별도 물리 주변기기의 Mac 전달, 실제 커서 기반 정확한 경계 판정, KeyBridge 입력 소유권 공존까지 해결했다는 뜻은 아니다.

범위: 공식 저장소의 관련 코드·문서만 읽었다. 실행·설치·기기 조작·권한 변경·테스트는 하지 않았다. 아래 Android 16/TB522FU 지원 여부는 미검증이다.

| 구현 | 반환/커서 기준 | Android 입력 경로 | 현재 요구와 다른 범위 |
| --- | --- | --- | --- |
| Ampersand (`crossinput`) | Mac의 virtualAxisPosition에 요청·수락 이동량 누적. 반환 방향은 요청 의도 사용 | ADB/app_process helper의 상대 UHID 마우스; 대상에 따라 InputManager fallback | 입력은 Mac→Android. Android 물리 마우스·키보드→Mac은 명시적 비목표 |
| UniControlHub | Synergy 가상 화면 좌표를 상대 HID 이동으로 변환. getCursorPos는 (0,0) 반환 | AOA 또는 ADB helper의 UHID | Android 실제 시스템 커서 반환 피드백이 이 경로에 없다. 별도 Android 물리 입력 수집/KeyBridge 소유권 계약도 확인되지 않음 |
| scrcpy | UHID 모드에서는 상대 이동 전달; Mac 복귀는 capture 단축키 | 공식 UHID/AOA 마우스·키보드 경로 | 자체 화면 경계 자동복귀 솔루션으로 확인한 것이 아니라, 입력 전달 기반으로 참고 가능 |

## 소스에서 확인한 핵심

**Ampersand:** 고정 SHA `cc17d614e1f41909d4f5e0fc96c744061bc5d257`. [EdgeSwitchStateMachine.swift](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/apps/macos/Sources/EdgeSwitch/EdgeSwitchStateMachine.swift#L74)는 실제 Android 커서 좌표 대신 경계에 수직인 가상 축을 사용한다. [실제 누적 코드](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/apps/macos/Sources/EdgeSwitch/EdgeSwitchStateMachine.swift#L256)는 반환 방향의 요청 delta와 안쪽 방향의 전달 delta를 선택해 누적하고 hysteresis를 넘으면 Mac으로 돌아온다.

[UHID injector](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/android/helper/app/src/main/java/com/crossinput/helper/UhidPointerInjector.kt#L58)의 수락 delta는 기록한 HID report 이동량이다. 실제 화면 커서 위치나 Android 가속·다른 물리 마우스 움직임의 측정값이 아니다. [설계 기록](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/docs/adr/ADR-0010-uhid-desktop-pointer-routing.md#L87)도 숨겨진 커서 위치 동기화 API 도입을 거부한다. 반환에 own AccessibilityService/관찰 권한이 필요한 경로는 이 코드에서 확인되지 않는다. 다른 InputManager fallback은 hidden API를 사용하므로 “모든 hidden API가 없다”는 주장은 아니다.

[README](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/README.md)는 Mac→Android 단방향 입력과 Android→Mac 비목표를 명시하고, 주요 기기 증거를 Galaxy S10 5G/Android 12로 제한한다. 기능 설명·작성자 기기 증거는 이번 Lenovo Android16 검증과 다르다.

**UniControlHub:** 고정 SHA `1a77f8115e8734a4036d9fa1da96f7bbe25eed4e`. [ClientScreen.mouseMove](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/lib/app/client/client_screen.dart#L60)는 Synergy x/y와 이전 값을 빼서 상대 HID report를 만든다. 같은 파일의 [getCursorPos/getShape](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/lib/app/client/client_screen.dart#L110)는 실제 Android 위치가 아니라 (0,0)과 설정된 가상 화면 크기를 반환한다. [leave](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/lib/app/client/client_screen.dart#L163)도 가상 Synergy 전환에 따른 후처리다.

[UHID transport](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/lib/app/communication/uhid/uhid_device.dart#L43)는 생성/쓰기/닫기 report를 helper로 보낸다. [Android helper](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/uni_hub_android_server/UniHubServer.java)는 이 report를 UHID에 적용한다. 이 입력 전달·가상 화면 경로에서 own 접근성 관찰 설정은 확인되지 않는다. 제한된 관련 소스 읽기이므로 전체 저장소의 모든 외부 라이브러리 API 부재를 증명한 것은 아니다.

**scrcpy:** 고정 SHA `19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac`. [mouse_uhid.c](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/app/src/uhid/mouse_uhid.c#L30)는 상대 HID report를 보내며 relative_mode=true로 설정한다. [공식 mouse 문서](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/doc/mouse.md#L28)는 UHID/AOA에서 호스트 포인터 capture와 단축키 복귀를 설명한다. 이는 실제 Android 커서 관찰·자동 경계복귀·Android 물리 주변기기 역방향 소유권을 대신하지 않는다.

## 재사용과 아키텍처 판단

라이선스는 [Ampersand Apache-2.0](https://github.com/luceat-lux-vestra/crossinput/blob/cc17d614e1f41909d4f5e0fc96c744061bc5d257/LICENSE), [scrcpy Apache-2.0](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/LICENSE), [UniControlHub GPL-3.0](https://github.com/rohitsangwan01/uni_control_hub/blob/1a77f8115e8734a4036d9fa1da96f7bbe25eed4e/LICENSE)로 표시된다. 실제 코드 재사용은 해당 LICENSE/NOTICE와 의존성 경계를 검토할 작업이며, 이 비교가 재사용 법률 판단을 대신하지 않는다.

가장 직접적인 참고는 Ampersand의 가상 축·hysteresis·로컬 복구와 scrcpy의 상대 UHID 입력이다. **Mac 주변기기 공유로 범위를 한정하고 반환의 좌표 모델 차이를 받아들이면 기존 방식으로 접근할 수 있다.** Android 물리 주변기기까지 역방향 공유하고 KeyBridge를 유지하며 실제 Android 커서에 맞춰 이동하려는 현재 요구는 별도 입력 관찰·소유권·복구 검증이 필요하다. 조사한 저장소에서 그 조합의 현재 기기 보장을 찾은 것은 아니다. 기존 구현 존재와 현재 Leftcar 실패 원인 해결은 서로 다른 증거다.
