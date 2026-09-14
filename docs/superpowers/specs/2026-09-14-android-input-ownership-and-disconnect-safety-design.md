# Android 입력 소유권 전환과 연결 종료 안전 설계

**상태:** 대화에서 방향 승인됨. 구현 전 문서 검토 대기.

## 1. 목표

Lenovo Android 태블릿의 물리 키보드와 마우스를 Android와 Mac 사이에서
명시적으로 넘긴다. 한 시점에는 한 운영체제와 한 커서만 입력을 소유해야 한다.
연결이 끊기거나 창이 사라지면 모든 원격 키와 버튼을 해제하고 Android 소유로
돌아오며, Mac을 잠그거나 재우지 않는다.

완료된 패키지는 다음 기존 동작도 유지한다.

- Host 대시보드 창을 닫으면 프로세스와 스트리밍은 유지되지만 Dock 아이콘은 숨긴다.
- 스트림 화면의 불필요한 `?`, `ABC`, `✕` 칩은 다시 만들지 않는다.
- 일반 터치 제스처와 소프트키보드 입력은 기존 원격 입력 경로를 유지한다.

## 2. 확인된 현재 문제

### 2.1 커서 소유권이 둘이다

`StreamActivity`는 Host의 `LCD1` 커서 오버레이를 켠 동시에, 물리 마우스가
움직이면 Android `TYPE_ARROW` 포인터도 다시 켠다. 따라서 Mac 커서와 Android
커서가 동시에 보인다.

### 2.2 키보드 소유권이 암묵적이다

KeyBridge 접근성 서비스는 앱보다 먼저 외부 키보드 이벤트를 보고 일부 조합을
소비한다. 반면 Leftcar는 스트림 창이 열려 있으면 대부분의 키를 곧바로 Mac으로
보낸다. 둘 사이에는 현재 입력 소유자를 알리는 interface가 없다.

### 2.3 순간 끊김이 Mac 잠금으로 이어진다

현재 Host 설정의 `lockOnDisconnect`는 켜져 있었고, `ControlServer`는 마지막
세션이 사라지면 종료 이유와 관계없이 `CGSession -suspend`를 실행한다. 감사
기록에서 실제 `screen_locked / last_session_ended` 이벤트를 확인했다. 네트워크
순간 끊김과 사용자의 보안 의도를 구분할 수 없는 옵션이므로 유지하지 않는다.

## 3. 선택한 접근과 대안

### 선택: 명시적 입력 소유권 module + KeyBridge 바인딩

Leftcar Android에 작은 `InputOwnershipController` interface를 두고, 실제
Pointer Capture, 커서 표시, KeyBridge 연동, 원격 입력 해제를 adapter로
분리한다. KeyBridge는 Leftcar가 원격 소유권을 획득한 동안만 새 물리 키 시퀀스를
그대로 통과시킨다.

이 방식은 같은 Leftcar 스트림 창 안에서도 클릭 전과 후를 구분하며, 연결 해제와
프로세스 종료 때 자동 복구할 수 있다.

### 기각: Leftcar 패키지 전체를 항상 KeyBridge 통과 모드로 만들기

구현은 작지만 스트림 창을 연 순간부터 KeyBridge 설정이 사라진다. 사용자가
마우스를 Mac에 넘기기 전에는 Android가 입력을 소유해야 한다는 요구를 만족하지
못한다.

### 기각: ADB/Shizuku로 매 전환마다 입력 설정 바꾸기

일반 키 이벤트 라우팅에 관리자 경로가 필요하지 않으며, Shizuku 연결 상태가
입력 소유권의 필수 조건이 된다. ADB/Shizuku는 Lenovo OEM 예약 단축키처럼
공개 Android 경로로 처리할 수 없는 기능에만 남긴다.

## 4. 입력 소유권 module

`InputOwnershipController`는 다음 세 상태만 외부에 드러낸다.

- `LOCAL_ANDROID`: 기본 상태. 물리 마우스와 키보드는 Android가 소유한다.
- `ACQUIRING_REMOTE`: 첫 마우스 클릭 뒤 KeyBridge 통과 모드와 Pointer Capture를
  요청하는 짧은 중간 상태다. 첫 클릭은 소유권 전환에만 쓰고 Mac 클릭으로 보내지
  않는다.
- `REMOTE_MAC`: Pointer Capture가 실제로 승인된 상태다. 일반 물리 키보드와
  마우스 입력을 Mac으로 보낸다.

Controller의 interface는 상태 이벤트를 받고 실행할 effect 목록을 반환한다.
Android framework 객체, JNI, Binder를 직접 소유하지 않는다.

주요 입력 이벤트는 첫 물리 마우스 클릭, Pointer Capture 획득/상실, Esc,
Mac 화면 경계 밖으로 향하는 상대 이동, Host 입력 허용 해제, 창 포커스 상실,
Surface 종료, Activity 종료다.

항상 지켜야 하는 불변식은 다음과 같다.

1. `REMOTE_MAC`일 때만 물리 키보드와 캡처된 마우스를 Mac으로 보낸다.
2. 원격 상태를 벗어나는 모든 경로에서 `ViewerNative.releaseInput`을 먼저 보내고
   Pointer Capture와 KeyBridge 통과 모드를 해제한다.
3. Pointer Capture 실측 상태가 Controller의 최종 진실이다. 요청 성공을 추정하지
   않는다.
4. Power, Android 보안 화면, 시스템 전용 키는 가로채거나 원격 전송한다고
   약속하지 않는다.

## 5. 마우스와 커서 흐름

### Android 소유 상태

- 물리 마우스를 움직이면 Android 포인터 하나만 보인다.
- Host에는 커서 분리 옵트인을 유지해 영상에 Mac 커서가 구워지지 않게 하지만,
  Leftcar의 Mac 커서 오버레이는 숨긴다.
- 물리 마우스 이동과 버튼은 Mac으로 보내지 않는다.

### Mac 소유 상태

- 사용자가 스트림 표면을 첫 클릭하면 KeyBridge 준비 후
  `requestPointerCapture()`를 호출한다.
- Pointer Capture가 확인되면 Android 포인터를 숨기고 Mac `LCD1` 오버레이만
  표시한다.
- 캡처 이벤트의 `AXIS_RELATIVE_X/Y`를 첫 클릭 위치에서 시작한 정규화 좌표에
  누적한다. 기존 LCI1 절대 좌표 protocol은 바꾸지 않는다.
- 누적 좌표가 Mac 화면 경계에 있고 이동이 바깥쪽을 향하면 원격 소유권을
  해제한다. Esc도 같은 해제 경로를 사용한다.
- 버튼, 스크롤, 움직임은 기존 신뢰성/합치기 정책을 그대로 사용한다.

터치스크린 제스처는 기존처럼 Mac을 직접 조작한다. 터치 중에는 Android 물리
포인터가 표시되지 않으므로 Mac 커서 오버레이를 보여 주고, 다음 로컬 물리 마우스
이벤트에서 다시 숨긴다.

`커서 오버레이` 사용자 설정은 제거한다. 두 커서를 함께 보이게 하거나 원격 상태의
커서를 완전히 숨기는 조합을 허용하지 않고, 입력 소유권에 따라 올바른 한 개를
자동 선택한다. 예전 저장값은 읽지 않아도 오류 없이 기본 자동 정책으로 이동한다.

## 6. KeyBridge 연동

KeyBridge는 export된 바인딩 service 하나를 제공하고, Leftcar는 명시적
ComponentName으로 연결한다. 바인더 transaction에서 호출 UID가 실제
`leftcar.ll3.kr` 패키지인지 확인한다. 이 service는 키를 기록하거나 주입하지
않고, 접근성 remapper에 현재 원격 소유권만 전달한다.

KeyBridge의 `RemotePassthroughGate`는 물리 키 down/up 시퀀스의 소유자를
기억한다.

- 전환 전에 KeyBridge가 소비한 down은 대응 up까지 KeyBridge가 소비한다.
- 원격 상태에서 Android로 통과한 down은 대응 up까지 계속 통과시킨다.
- 새로운 down만 현재 소유권 정책을 따른다.
- 원격 획득 시 KeyBridge가 합성해 잡고 있던 출력은 먼저 해제한다.
- Leftcar 연결이 죽거나 unbind되면 자동으로 로컬 모드로 복구한다.

KeyBridge가 설치되지 않은 기기에서는 Android가 원래 키를 Leftcar에 전달하므로
원격 모드를 허용한다. KeyBridge 패키지는 설치되어 있지만 연동 service가 없는
구버전이면 모호한 키 상태를 만들지 않고 원격 획득을 거부하며 업데이트 안내를
보여 준다.

## 7. 연결 종료와 Mac 잠금 정책

`종료 시 잠금`은 완전히 삭제한다.

- Host UI의 토글과 설명을 삭제한다.
- Tauri command와 `HostSettings.lock_on_disconnect`를 삭제한다.
- `lock.rs`, `CGSession -suspend`, `pmset displaysleepnow`, Windows
  `LockWorkStation` 호출을 삭제한다.
- 세션 GC, 명시적 종료, 뷰어 종료, 네트워크 오류 어디에서도 Mac 잠금을 호출하지
  않는다.
- 기존 `settings.json`의 `lockOnDisconnect` 키는 무시하고, 다음 설정 저장 때
  쓰지 않는다. 설치 시 현재 사용자 설정의 키도 제거한다.

연결 종료가 해야 하는 일은 원격 키·버튼 해제, Pointer Capture 해제, KeyBridge
로컬 모드 복귀, 커서 단일화, 스트림 자원 정리뿐이다.

## 8. 오류 처리

- Host 입력 허용이 꺼져 있으면 원격 소유권을 획득하지 않는다.
- Pointer Capture가 거부되거나 즉시 상실되면 KeyBridge를 바로 로컬로 돌리고
  원격 입력을 해제한다.
- KeyBridge 바인더가 죽으면 Leftcar도 원격 상태를 끝낸다.
- Activity 포커스 상실, Surface 교체, 재연결, `onPause`, `onDestroy`는 모두 같은
  idempotent 해제 interface를 호출한다.
- 해제는 몇 번 호출돼도 안전해야 하며, 다른 앱이나 로컬 Mac 입력이 누른 키는
  해제하지 않는다.

## 9. 검증

### 자동 검증

- `InputOwnershipController` 상태 전이와 effect 순서를 JVM 단위 테스트한다.
- 상대 좌표 누적, 경계 이탈, 첫 클릭 비전송, Esc 해제를 테스트한다.
- `RemotePassthroughGate`의 전환 중 down/up 보존을 KeyBridge 단위 테스트한다.
- Host의 모든 teardown 경로가 잠금 callback을 갖지 않는지 테스트와 정적 검색으로
  확인한다.
- Leftcar/KeyBridge Android 단위 테스트와 빌드, Host Rust 테스트, Swift 입력
  테스트를 실행한다.
- React/TSX 설정 UI가 바뀌므로 저장소 루트에서 React Doctor `100 / 100`,
  typecheck와 관련 테스트를 다시 실행한다.

### 실제 기기 검증

Lenovo TB710FU에 새 KeyBridge와 Leftcar를 설치하고 다음을 확인한다.

1. 스트림 열기 전과 첫 클릭 전에는 KeyBridge의 기존 Android 설정이 동작한다.
2. 첫 클릭 뒤 `dumpsys input`에서 Pointer Capture가 켜지고 Mac 커서 하나만 보인다.
3. 일반 키, 수정키 조합, 마우스 버튼, 휠이 Mac에 전달된다.
4. Esc와 Mac 화면 경계 이탈 뒤 Pointer Capture가 꺼지고 Android 커서와
   KeyBridge 설정이 복귀한다.
5. Wi-Fi 순간 끊김, 스트림 창 닫기, 앱 강제 종료에서 Mac이 잠기지 않고
   원격 키/버튼이 남지 않는다.
6. Host 창을 닫아도 스트리밍은 유지되며 Dock 아이콘이 사라진다.

물리 Power 키와 OEM/보안 예약키는 Android 정책상 완전 전달을 보장하지 않는다.
ADB 입력 명령은 물리 외부 키보드와 다른 경로이므로 물리 검증을 대체하지 않는다.

## 10. 산출물과 범위

- `/Applications/Leftcar Host.app` 교체 설치
- Lenovo TB710FU에 새 Leftcar와 KeyBridge 설치
- 새 Leftcar APK를 `/Users/loopy/Downloads`에 복사하고 해시 확인
- KeyBridge APK도 재설치 가능한 파일로 함께 보관
- 커밋과 push는 별도 요청 전에는 하지 않는다.

비목표는 자동 hover만으로 Pointer Capture에 진입하는 동작, root 기반 전역 입력,
Mac 커서 모양 bitmap 동기화, iPadOS 전역 키 리매핑이다.
