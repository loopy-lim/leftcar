# 기존 설치본 전체 E2E 실행 계획

**상태:** 사용자의 지시로 실행을 중단했다. 이 체크리스트는 전체 E2E 통과 기록이 아니다. 테스트가 연 스트림을 닫고, 테스트 설정과 입력 승인을 원래 값으로 복원했으며 공유 큐를 정리했다. 이후 설치·기기 실행을 재개하지 않고 [연결 안정성 소스 수정과 자동 검증](../evidence/2026-10-03-connection-reliability-verification.md)으로 범위를 변경했다.

**Goal:** 기존 macOS Host 0.1.10과 Android Viewer 0.1.10의 정상 연결부터 실패·복구·종료까지 실제 앱 경로를 시험한다.

**대상:** `/Applications/Leftcar Host.app` SHA256 `e799f515cc2833d4368b89ce814680a80cf9f7cbf532d4cb6b64e40eac395524`, TB710FU/Android 16의 `leftcar.ll3.kr` APK SHA256 `53dad236d7b1ba4c2f30c38b706281cec42d99abf729357dcdc5bf372540cfdd`. 현재 작업 트리와 설치본 소스의 동일성을 가정하지 않는다.

**범위:** 현재 연결된 Wi-Fi와 기존 앱의 사용자가 허용한 권한으로 실행 가능한 모든 아래 흐름. 설치·초기화·개인 파일 변경·기존 paired credential 삭제·가상 디스플레이 변경을 하지 않는다. 테스트 세션·창·공개 fixture만 만들고 설정은 원래 값으로 돌린다. 사용자의 기존 세션과 다른 앱은 중단하지 않는다.

**근거:** [안전성 요구사항](../specs/2026-10-03-safety-and-ux-system-design.md), [기존 Android smoke](../evidence/2026-10-03-installed-android-adb-verification.md). 이전 smoke와 자동 검증을 아래 새 실행의 통과로 대신하지 않는다.

## 실행과 판정

- [ ] 대상 바이너리·APK·현재 기기·기존 세션·설정·input 승인·개인 공유 큐를 읽고 baseline을 기록한다. credential과 클립보드 원문은 출력하지 않는다.
- [ ] 기존 연결을 앱에서 해제하고 탐색·선택·기존 credential 재연결 후 화면 목록이 다시 표시되는 것을 확인한다.
- [ ] 단일 주 화면을 열어 동일 incarnation의 수신·디코드·Surface 제출 증가를 확인한다. 단일 화면 닫기에서 native ACK·renderer exit를 확인한다.
- [ ] 서로 다른 품질 프로필을 테스트의 새 연결에 적용하고 실제 stream 초기화 값을 관찰한 뒤 기존 프로필을 복원한다.
- [ ] 기존 UI가 허용하면 같은 물리 source에 테스트 영상 창을 두 개 열어 독립 instance와 출력 증가를 확인한다. 한 창을 닫아 다른 창이 진행하는지, 마지막 창 종료가 완료되는지 확인한다.
- [ ] 테스트 영상 창을 배경화·다시 전면화하고 Surface/decoder 수명 및 출력 재개를 확인한다. 테스트 앱 전체의 Back 복귀도 확인한다.
- [ ] 테스트가 소유한 Host 실행의 일시 정지 또는 앱이 제공하는 연결 해제로 연결 실패와 원인 표시를 확인한다. 복구 후 같은 사용자 앱에서 재연결·새 영상 출력을 확인한다. 실행 소유권이 없으면 기존 사용자 Host를 중단하지 않고 다른 안전한 실패 경로를 사용한다.
- [ ] 토큰 없는 읽기 요청·invalid token·malformed handshake·초과 크기 미인증 프레임의 거부와 연결 종료를 검증한다. 상태 변경 명령에 대한 무인증 테스트는 불완전 args와 테스트 resource만 대상으로 한다.
- [ ] Host의 공개 fixture를 큐에 등록하고 Android UI에서 수신한다. 실제 수신 bytes/hash를 확인할 정상 UI/공유 경로가 있으면 비교한다. 없으면 UI 완료를 byte 검증의 통과로 확대하지 않는다. 테스트 queue 행만 제거한다.
- [ ] Android의 공개 fixture를 SAF picker로 선택해 Host로 전송한다. Host 수신 파일의 byte length와 SHA256을 비교한다. 사용자 파일과 같은 이름으로 덮어쓰지 않는다.
- [ ] 양쪽의 private clipboard가 전송되지 않도록 보존·공개 fixture 준비를 먼저 한다. 준비가 가능한 정상 경로에서만 양방향 sync·off 상태 미전송을 확인한 뒤 기존 설정과 clipboard를 복원한다.
- [ ] 원격 입력은 테스트 전용 문서/텍스트 필드에만 보낸다. 입력 off에서 nonce 미입력, on에서 실제 Viewer 경유 입력을 확인하고 기존 persistent device 승인을 복원한다. 새 OS 권한은 허용하지 않는다.
- [ ] 기존 앱의 지원 UI에서만 설정 저장·재마운트와 잘못된 입력의 오류·의도 보존을 확인한다. 유효하지 않은 설정을 그대로 저장하거나 사용자의 기존 credential을 손상시키지 않는다.
- [ ] 원래 설정·권한·기존 credential·사용자 큐와 테스트 소유 세션 정리를 비교하고 각 항목을 PASS/FAIL/UNAVAILABLE로 기록한다.

## 외부 실행 조건

USB/AOAP cable unplug·blocking driver 복구, 새 카메라·macOS privacy 권한 허용, Windows, 다른 Android 기기, 광학 지연·물리 패널 FPS와 장시간 soak는 현재 환경의 실행 조건을 확인한 뒤 별도 판정한다. 실행하지 않은 항목을 PASS로 기록하지 않는다. 전체 E2E라는 표현도 위 실제 시험 항목과 외부 조건을 함께 제시한다.

**기록 경로:** `/private/tmp/leftcar-full-e2e-20261003-7_0j14bj/`. 공개 fixture 두 개의 SHA256과 초기 Host 프로필 파일 hash는 이미 이 경로에 기록했다. CLI/로그·UI hierarchy를 우선하고 화면 캡처는 해결되지 않은 시각적 주장에만 사용한다.
