# Leftcar 개선 재검증과 유사 프로젝트 성능 비교

기준일: 2026-09-12 · 저장소: `/Users/loopy/dev/ll3/leftcar` · HEAD: `068b6628df5dc57f264446f8ec51215b37c51b6f`

## 판정

**아직 모두 개선되지는 않았다.** 이전 전체 감사의 29개 항목을 현재 소스와 다시 대조한 결과, **수정 확인 10개, 부분 해결 17개, 미완료 2개**다. 이전 감사 이후 관련 변경은 6개 커밋으로 반영됐으며 현재 작업 공간은 깨끗하다.

여기서 ‘수정 확인’은 지적한 코드 결함이 수정됐다는 뜻이다. Android/Windows 실기기, 접근성 조작, 장시간 스트리밍까지 통과했다는 뜻은 아니다. ‘부분 해결’에는 원래 재현은 해결됐지만 같은 기능의 다른 취소·복구 경계가 남은 경우와, 여러 개선 요구 중 일부만 구현된 경우가 포함된다. 이 숫자를 성능 향상률이나 제품 완성도로 해석하면 안 된다.

비교 대상은 데스크톱→Android 전송이라는 경로가 가까운 **Sunshine + Moonlight**, 저지연 화면 전송 설계를 참고할 수 있는 **scrcpy**다. scrcpy는 Android→컴퓨터로 방향이 다르므로 절대 FPS나 지연을 직접 비교하지 않았다. 동일 장치·화질·네트워크의 A/B 실측 없이 어느 제품이 몇 배 빠르다는 결론은 내리지 않는다.

## 다시 실행한 검사

| 검사 | 결과 | 해석 |
|---|---|---|
| 루트 typecheck | 통과 | 이제 Expo Router `app/`와 Host 개별 검사 포함 |
| TypeScript 테스트 | **50파일, 612개 통과** | 이전 589개에서 회귀 테스트 추가 |
| Rust workspace | **390개 통과, 1개 ignored** | ignored는 vector 출력용 테스트 |
| 별도 Host workspace | **172개 통과** | 이전 154개보다 확장 |
| workspace / Host Clippy | 모두 통과 | 경고를 오류로 취급하는 조건 |
| workspace fmt | 통과 | 이전 실패 해소 |
| TS/Kotlin 아키텍처 검사 | 통과 | Rust 의존성 fixture도 workspace 테스트에 포함 |
| React Doctor | **133파일, 100 / 100, 0 issues** | 의미적 경합·기기 UX까지 보증하지는 않음 |
| Swift 전체 소스 + policy 테스트 | 컴파일·실행 exit 0 | 내부 dual AVE probe는 `create(-12903)`로 실패; 하드웨어 지원 통과 아님 |
| Swift 전체 소스 + RTX ring 테스트 | 컴파일·실행 exit 0 | 현재 AU 상한 예외를 허용하는 테스트도 포함 |
| Swift 전체 소스 + split 테스트 | **컴파일 성공, 실행 실패(exit 133)** | debug 재실행에서 `SplitPipelineTests.swift:722: Fatal error: carrier pixel buffer` 확인 |
| 추가 실제 소스 기반 재현 | 잔여 결함 확인 | Host 3개 안전 조건 실패, Viewer 취소·전환·모달 경계, split/NACK/FEC probe |
| 깨끗한 환경의 Android 설정 의존성 | 실패 재현 | Gradle 설정의 Node 해석식이 `react-native/package.json`을 못 찾음; 전체 CI 실행을 대신하는 결과는 아님 |

검증 기록은 [evidence-2026-09-12/README.md](/Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12/README.md)에 보관했다. Android APK 전체 빌드, 실제 GitHub Actions 실행, Windows GPU, TalkBack/Tauri 포커스, 실기기 4K60 및 발열·배터리·glass-to-glass는 이번에 검증하지 않았다. 생성 코드 재생성도 이번 검사에는 포함하지 않았다.

## 이전 29개 항목 전체 대조

| ID | 주제 | 판정 | 수정된 부분 / 남은 부분 |
|---|---|---|---|
| F01 | UDP 성공 길이 계약 | **수정 확인** | 현재 암호화 송신으로 평문 1200B → 반환 1200B → 수신 1224B 확인 |
| F02 | 시작·재구성 중 정지/철회 | **부분** | 대기 중 철회·삭제·동시 재구성 보호 추가. 등록 직전 철회와 Host 강제 정지 후 재구성 부활은 재현됨 |
| F03 | AOAP 기기 identity | **수정 확인** | 인증된 기기 ID를 dispatch까지 전달 |
| F04 | 다른 기기의 세션 제어 | **수정 확인** | 조회 필터·정지·재구성 소유권 검사 및 회귀 테스트 |
| F05 | 호스트별 인증 토큰 | **부분** | A→B→A 해결. A 요청의 늦은 401이 현재 B 토큰을 지우는 경계 잔존; endpoint와 안정적 identity 매핑도 별도 |
| F06 | 늦은 연결 완료 | **부분** | disconnect/새 연결 뒤 늦은 소켓 폐기 해결. 자동 재연결이 새 사용자 선택을 취소시키는 경계 잔존 |
| F07 | 클립보드 취소 | **부분** | 끄기/stop 이후 5개 await 경계에서 새 부작용 0건. 호스트 교체에 따른 취소 연결은 없음 |
| F08 | 인증 전 무제한 입력 | **수정 확인** | handshake 16KiB, 큰 명령 12MiB, 동시 연결 64개로 제한. 토큰 인증 전 별도 작은 예산은 추가 개선 권장 |
| F09 | split 만료 뒤 참조 체인 | **부분** | 다음 입력의 paired IDR 요청 추가. 이미 제출된 delta의 차단·세대 무효화·idle carrier 연결은 미흡 |
| F10 | 설정 저장·토글 | **부분** | backend 직렬화·임시 파일 교체 해결. 한 토글 조작이 다른 토글의 초기값 로드까지 막는 UI 오류 잔존 |
| F11 | 페어링 영속 실패 | **부분** | credential 저장 실패 전파 해결. 기기 metadata 저장 실패는 성공 응답 후 재시작 인증 상실 |
| F12 | 감사 로그 파일 권한 | **수정 확인** | 생성 시 0600, 첫 기록·회전 직후 테스트 |
| F13 | 입력·source 승인 정책 | **부분** | 자동 입력의 문서 표기 추가. source 승인 미구현, README 내부의 명시 승인 설명도 불일치 |
| F14 | NACK 유예와 큐 상한 | **부분** | 이미 설정된 유예의 3프레임 조기 배출 해결. 첫 수신 배치에서 유예 설정 전에 hole을 버리는 경계 잔존 |
| F15 | 디코더 자원 예산 | **부분** | capacity 초과 일반 창 거부. 시작 전 원자적 예약·기기 능력 연결은 없음 |
| F16 | Windows idle 출력 처리 | **수정 확인** | 새 캡처가 없어도 encoder 출력을 pump·송신. Windows 실기기 판정은 대기 |
| F17 | RTX 캐시 복사·상한 | **부분** | 내부 Dictionary 복사 비용 개선. 현재 AU는 여전히 512개 상한 예외 |
| F18 | 페어링 취소 | **부분** | QR 응답 대기 중 취소 후 토큰 저장 방지. PIN과 connect 이후 화면 이동의 취소 수명 잔존 |
| F19 | 카메라 영구 거부 복구 | **수정 확인** | 설정 열기, AppState 복귀 시 권한 재조회. 기기 조작 미검증 |
| F20 | 모달 포커스·Esc | **부분** | showModal·포커스 복원 추가. React cancel 전파로 부모·자식 동시 닫힘 재현 |
| F21 | 접근성·터치 영역 | **부분** | OTP·품질 선택 의미 개선. HUD 높이 48dp에 간격 40dp로 버튼 겹침 |
| F22 | 복사·승인 오류 표현 | **수정 확인** | 성공 후 완료 표시, 실패 배너·행 유지. 실제 UI 실패 주입은 미검증 |
| F23 | 전체 검사·CI 범위 | **부분** | Host/Expo/Swift/React gate 추가. Android job의 JS 설치 누락, 로컬 split 테스트 실행 실패 |
| F24 | native 빌드와 APK 연결 | **부분** | Gradle→Cargo 의존성 추가. Gradle 입력에서 공유 Rust crate 변경이 빠져 오래된 .so 재사용 가능 |
| F25 | fmt·Clippy 실패 | **수정 확인** | 새 검사 모두 통과; toolchain 명시 |
| F26 | 상속·target 의존성 누락 | **수정 확인** | workspace 상속과 target.dependencies 처리, 위반 fixture 추가. 별도 Host/의미적 책임 검사 확대는 후속 구조 과제 |
| F27 | 실제 세션 정책의 구조 통합 | **미완료** | 실제 Host가 fake orchestration 중심 host-core를 쓰지 않는 구조 유지; 취소·소유권 정책 통합은 미실행 |
| F28 | 문서·버전·배포 신뢰 | **부분** | 보안·EVIDENCE 표현과 버전 설명 개선. release 서명·산출물 provenance·업그레이드 검증은 미완료 |
| F29 | 성능 수용 기준 실측 | **미완료** | 현재 변경에 대한 동일 조건 전후 측정·장시간 수용 증거 없음 |

F26은 원래 발견한 상속·target 누락의 수정을 인정했다. 모든 아키텍처 개선 제안을 끝내야 해당 결함이 해결된 것으로 보겠다는 의미는 아니다. F08도 ‘무제한’ 결함의 수정과 추가 메모리 예산 강화 제안을 구분했다.

## 먼저 닫아야 할 잔여 결함

### 1. 권한 철회와 Host 정지가 최종 상태를 보장하지 못한다 — P1

[control.rs:1629](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src-tauri/src/control.rs:1629)의 마지막 paired 검사와 이후 세션 등록 사이에 revoke가 들어갈 수 있다. 실제 ControlServer 소스의 backend 입력 권한 조회 경계에 철회를 주입한 결과 `still_paired=false`, `live_sessions=1`, `stops=0`, 응답 `ok:true`였다. 기존 테스트는 마지막 재검사보다 앞에서 발생한 철회만 검증한다.

또한 [force_stop_session](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src-tauri/src/control.rs:1293)은 정지 상태의 항목을 map에 남긴다. [재구성 완료](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src-tauri/src/control.rs:1007)는 항목의 존재만 확인하고 종료 표시를 지워 버린다. 대기 중 Host 정지를 주입하자 `backend_released=false`, `terminal_error=None`으로 되살아났다.

기기 권한 세대와 세션 종료 세대를 최종 등록·replacement commit에 함께 검사하고, 실패 시 새 backend를 정리해야 한다. map 삭제 경로뿐 아니라 Host 정지로 남긴 항목도 테스트해야 한다.

### 2. split 프레임 폐기는 여전히 다음 delta의 참조를 깨뜨릴 수 있다 — P1

[split 만료 처리](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Split/CaptureSession+Split.swift:260)는 다음 admission에 키프레임을 요구한다. 현재 flow/lifecycle/wire 소스로 재생한 상태에서는 이미 admission된 후속 delta가 계속 유효하고, 폐기된 프레임은 wire 번호 공백을 만들지 않는다. `pendingAccepted=true`, `generationUnchanged=true`, wire `0→1`, 다음 새 admission만 `requestKeyframe=true`였다.

IDR이 나올 때까지 이미 대기 중인 종속 delta를 차단하거나 generation을 바꾸어 참조 체인을 정리해야 한다. 정적인 화면에서 새 캡처가 없어도 보관한 프레임으로 복구할 수 있도록 기존 carrier 경로까지 연결해야 한다. 이 결론은 상태 정책 재현과 실제 callback/queue 경로 추적이며, 영상 손상을 실기기에서 촬영한 증거는 아니다.

### 3. ‘현재 호스트’와 ‘실패한 요청의 호스트’가 섞인다 — P1/P2

[401 처리](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/src/connect-flow.ts:21)는 응답을 보낸 client 대신 현재 `controlTarget()`을 사용한다. A 요청을 기다리던 중 B로 바꾸고 A의 401을 처리하면 B 토큰이 지워졌다. [연결 세대](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/src/session.ts:69)도 사용자 선택과 자동 복구가 함께 증가시키므로, B 연결 도중 A 자동 복구가 시작되면 B가 취소됐다.

요청 시작 시 `{target, client, generation}`을 고정하고, 오래된 실패·자동 복구는 새 사용자 선택에 영향을 주지 않게 해야 한다. 클립보드도 같은 호스트 세대 교체에 취소를 연결해야 한다. 현재 재현은 닫힌 이전 client로 요청을 시도한다는 증거이며, 전송 성공이나 데이터 유출을 입증한 것은 아니다.

### 4. 페어링 metadata 실패도 성공으로 반환된다 — P2

[pairing.rs:371](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src-tauri/src/pairing.rs:371)는 metadata 저장 실패를 로그만 남긴다. credential 저장소는 정상이고 metadata 경로만 실패하도록 주입하자 `pairing_reported_success=true`, `authenticates_after_restart=false`였다. 두 저장이 모두 완료된 뒤 성공을 반환하고, 중간 실패 시 메모리 등록·새 credential을 정리해야 한다.

### 5. NACK 복구 기회를 첫 수신 배치에서 놓친다 — P2

활성화된 25ms 유예 동안 후속 프레임 3개를 보존하는 수정은 확인됐다. 하지만 [worker의 NACK tick](/Users/loopy/dev/ll3/leftcar/native/android-viewer/src/renderer/single_session/runtime/worker.rs:376)이 수신보다 앞에 있다. 첫 배치에서 부분 AU11과 완성 AU12·13·14가 처리되면 일반 큐 상한이 먼저 적용되어 `released=[12,13,14]`, `nacksSent=0`, hole 없음이 됐다.

완성 프레임을 비가역적으로 배출하기 전에 hole의 복구 가능성을 판단해야 한다. 유예·6프레임 강제 상한은 유지하면서, 사전에 유예를 설정하지 않은 실제 수신 배치 순서를 회귀 테스트에 넣어야 한다.

### 6. Android 빌드 경로의 재현성이 아직 부족하다 — P1/P2 검증 공백

[Android CI job](/Users/loopy/dev/ll3/leftcar/.github/workflows/ci.yml:70)은 Gradle을 추가했지만 해당 job에 Bun/JS 의존성 설치가 없다. [settings.gradle:5](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/settings.gradle:5)는 즉시 RN Gradle plugin과 Expo를 node_modules에서 찾는다. 동일 Node 해석식을 의존성 없는 임시 디렉터리에서 실행하면 `MODULE_NOT_FOUND`다. 다른 job의 설치는 새 runner에 전달되지 않는다.

[buildViewerNative 입력 목록](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/build.gradle:244)에는 android-viewer 소스·manifest·Cargo.lock만 있다. [실제 의존성](/Users/loopy/dev/ll3/leftcar/native/android-viewer/Cargo.toml:12)인 secure-channel, viewer-decoder, fec-core 등의 소스, 루트 Cargo 설정은 없다. 공유 crate만 바꾸면 Gradle이 UP-TO-DATE로 Cargo 자체를 생략할 수 있다. Cargo를 항상 호출해 자체 증분 검사를 맡기거나 해석된 전체 입력을 등록해야 한다.

Swift split 검사는 컴파일 성공 뒤 carrier pixel buffer 생성에서 중단됐다. 추가 probe에서 4×4·320×320 모두 현재 fixture의 Metal 호환 버퍼는 `CVPixelBufferCreate=-6662`였고 일반 CPU 버퍼는 성공했다. 이 환경의 Metal 호환 버퍼 생성 제약을 확인했으며, 실제 스트리밍 결함이나 GitHub runner에서도 같은 실패가 난다고 단정하지 않는다. 순수 정책 검사와 GPU 요구 fixture를 분리할 필요가 있다. 현재 기록을 ‘전체 Swift 테스트 통과’로 표시하면 안 된다.

### 7. UI 수정에도 새 경계가 남아 있다 — P2

- [Privacy.tsx:59](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src/Privacy.tsx:59)의 두 초기 로드 gate를 AND로 묶어, lock을 먼저 조작하면 curtain 저장값도 버린다. `curtainStored=true`인데 `curtainUi=false` 재현. 각 설정을 독립 판정해야 한다.
- [Modal.tsx:67](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src/Modal.tsx:67)의 cancel 처리는 preventDefault만 한다. 설치된 ReactDOM 19.2.3 dispatcher 재현에서 `closeCalls=[child,parent]`. 이벤트 전파 또는 최상위 모달 경계를 통제해야 한다. 실제 WebView 포커스는 미검증이다.
- [HUD 높이](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt:376)는 48dp지만 배치 Y는 52/92/132dp다. 최소 8dp가 겹친다. 별도 PopupWindow의 고정 좌표보다 실제 크기를 반영하는 공통 컨테이너가 적절하다.
- [디코더 admission](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/src/use-catalog-model.ts:522)은 완료된 streams snapshot에 의존한다. 시작 중 슬롯을 예약하지 않아 동시 admission의 합은 5/4가 될 수 있다. 기존 일반 클릭 차단은 완화책이지만 원자적 자원 보장은 아니다.

## 비교에서 찾은 추가 성능 개선점

아래 순서는 구현 규모와 근거의 확실성을 고려한 권장 순서다. F02/F09 같은 정확성 문제를 먼저 닫아야 최적화 측정이 의미가 있다. 예상 효과는 후보이며, 이미 측정한 것은 별도로 표시했다.

### A. FEC 복원이 불가능한 경우 할당 전에 반환 — 우선순위 높음, 작은 변경

Leftcar는 [복원 함수](/Users/loopy/dev/ll3/leftcar/native/android-viewer/src/media_datagram.rs:449)에서 shard를 포장·복사한 뒤 개수를 검사한다. 필요한 8개 중 3개만 있는 상태로 100회 호출한 실제 함수 실험에서 **할당 400회, 총 448,800B**가 발생했고 결과는 모두 복원 불가였다. 이는 라이브 스트림에서 관측한 빈도가 아니라 불필요한 작업을 분리한 fixture다.

Moonlight는 복원에 충분한 packet이 있는지 먼저 확인한다. [공식 RtpVideoQueue 구현](https://github.com/moonlight-stream/moonlight-common-c/blob/62e066388f1a1b133e0bee947b9a374311a3354b/src/RtpVideoQueue.c#L212-L234)

**개선:** 중복·유효 shard를 정확히 세고, 불충분하면 pack/clone 전에 반환. 이후에는 크기 검증을 유지한다. 완료 기준은 같은 fixture의 조기 반환 할당 0회, 기존 FEC vector 동등성, 재정렬·손실 조건의 수신 CPU와 p95 악화 없음이다. 이미 있는 GF 곱셈표와 모든 data가 도착했을 때 조기 반환은 다시 만들 필요가 없다.

### B. 오디오 실제 버퍼 목표와 압축을 분리해 개선 — 우선순위 높음/중간

[StreamAudioPlayer:113](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamAudioPlayer.kt:113)는 최소 크기를 `rate × channels × 2 / 10`, 즉 **100ms 분량 이상**으로 만든 뒤 60ms 목표에 적용한다. 따라서 주석의 약 60ms와 요청한 버퍼 용량이 다르다. 실제 청취 지연이 정확히 100ms라는 실측 결론은 아니지만, 낮은 지연 목표를 무효화하는 하한이다. idle poll도 최대 40ms를 기다린다.

scrcpy는 오디오의 목표 버퍼를 명시적으로 조절하며 기본 50ms를 문서화한다. Android는 `getUnderrunCount()`와 `setBufferSizeInFrames()`로 지연과 끊김 사이를 조정하는 방법을 제공한다. [scrcpy 오디오](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/doc/audio.md), [AudioTrack API](https://developer.android.com/reference/android/media/AudioTrack#setBufferSizeInFrames(int))

**개선:** 먼저 실제 버퍼 크기·재생 위치·underrun·무음 후 재개·영상과 음성 차이를 계측하고 기기별 목표를 조정한다. 무조건 버퍼를 줄이면 끊김이 늘어난다. poll을 이벤트/조건 대기로 바꾸는 실험도 가능하다.

별도로 현재 [오디오 송신](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Transport/CaptureSession+Audio.swift:59)은 PCM이다. 48kHz·2채널·16bit일 때 payload는 **1.536Mbps**다. scrcpy의 기본 Opus 방식을 참고해 예를 들어 128kbps 모드를 실험하면 payload 계산상 약 91.7% 절감 여지가 있다. 이는 헤더·암호화 비용을 뺀 계산이며, 실제 총 트래픽 감소나 음질·지연 개선을 측정한 수치는 아니다. [scrcpy codec/bitrate 설명](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/doc/audio.md#codec)

압축은 codec CPU·프레임 지연·협상 구현을 추가하므로 버퍼 수정과 별도 실험으로 진행해야 한다. 압축 전에도 Float32→중첩 배열→PCM→Data의 [변환 경로](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Transport/CaptureSession+Audio.swift:101)를 재사용 버퍼로 줄일 후보가 있다.

### C. 즉시 표시와 화면 주기 기반 표시를 선택 가능하게 비교 — 우선순위 중간

Leftcar는 [single 출력](/Users/loopy/dev/ll3/leftcar/crates/viewer-decoder/src/android/decoder.rs:481)에서 오래된 디코딩 출력을 버리고 최신 것을 즉시 표시한다. [split](/Users/loopy/dev/ll3/leftcar/native/android-viewer/src/renderer/presentation_sync.rs:342)은 현재 시각+1ms를 쓴다. 이 방식은 낮은 지연에 합리적이며, 이미 latest-frame 처리가 있으므로 ‘프레임 드롭을 새로 추가하라’는 제안은 아니다.

Moonlight는 Choreographer와 실제 vsync offset을 이용하는 bounded 표시 모드를 갖고 있다. scrcpy도 기본 무추가 영상 버퍼와 선택적 버퍼를 구분한다. [Moonlight 표시 정책](https://github.com/moonlight-stream/moonlight-android/blob/98c12bebffac592eb57cf25e9a4638b40aa2c17d/app/src/main/java/com/limelight/binding/video/MediaCodecDecoderRenderer.java#L948-L977), [scrcpy 영상 버퍼](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/doc/video.md#buffering)

**개선:** 60fps 스트림을 90/120Hz 패널에 표시하거나 수신 간격이 흔들릴 때 ‘즉시’와 ‘균일한 표시 간격’을 비교한다. 최대 대기 프레임 수를 제한하고 실제 디스플레이 주기를 사용한다. 가상의 60Hz 시간축으로 맞추는 방식은 피한다. 완료 기준은 표시 간격 분포, 입력→광자 지연, split 좌우 동기와 frame-age p95를 함께 비교하는 것이다. 부드러워져도 지연이 늘 수 있다.

### D. 실제 디코더별 설정과 자원 예약 — 우선순위 중간

[Leftcar decoder 설정](/Users/loopy/dev/ll3/leftcar/crates/viewer-decoder/src/android/decoder.rs:26)은 표준 low-latency/priority/operating-rate와 QTI H.264 일부 옵션을 이미 사용한다. Moonlight는 기기·codec 특성에 따른 옵션, 알려진 예외, 위험한 설정을 단계적으로 제거하는 retry를 관리한다. [Moonlight MediaCodecHelper](https://github.com/moonlight-stream/moonlight-android/blob/98c12bebffac592eb57cf25e9a4638b40aa2c17d/app/src/main/java/com/limelight/binding/video/MediaCodecHelper.java#L494-L594)

**개선:** 실제 지원할 MediaTek·Exynos 등에서 측정해 recipe를 추가하고 QTI 옵션의 부분 제거도 재시도한다. 슬롯 수, 해상도×FPS, split의 2 decoder 비용을 분리하고 시작/재구성 전 예약한다. 옵션 이름을 일괄 복사하거나 모든 기기에 최대 operating-rate를 주면 발열·호환성을 악화시킬 수 있다. configure 성공률, 첫 화면 시간, decode p95, 장시간 온도·전력을 기기별로 남겨야 한다.

### E. Windows 송신의 순간 버스트와 호출 수 — 우선순위 중간, Windows 한정

[Windows capture.rs:226](/Users/loopy/dev/ll3/leftcar/apps/host-desktop/src-tauri/src/windows_backend/capture.rs:226)는 AU의 data+parity를 연속 송신한다. macOS에는 이미 pacing·AU deadline이 있으므로 새로 필요한 플랫폼을 구분해야 한다.

Sunshine은 전송 배치 크기를 제한하고, 프레임 사이에도 pacing 시간을 이어가며 플랫폼별 batch 송신을 사용한다. [Sunshine stream.cpp](https://github.com/LizardByte/Sunshine/blob/0f7dcf3dc20b2dfee0912e07f422f812a42d1621/src/stream.cpp#L1770-L1813)

**개선:** Windows에 bounded pacing과 가능한 batch 경계를 도입할 가치가 있다. Sunshine의 고정 처리량 가정을 그대로 복사하지 말고 Leftcar의 bitrate/복구 예산과 연결한다. IDR 전송 시간 p95, receiver 불완전 AU, 송신 실패와 소켓 호출 수를 같은 Wi-Fi 조건에서 비교한다. 지나친 pacing은 프레임 나이를 늘릴 수 있다.

### F. RTX 복사 최적화는 효과가 보이지만 byte 예산은 남는다 — 우선순위 중간

현재 AU 저장소가 참조형으로 바뀌어 fragment마다 중첩 Dictionary가 복사되는 경로는 제거됐다. 고정 receipt의 최적화 빌드 실험에서 317/634/1268개 store+lookup은 약 **0.109/0.246/0.567ms**였다. 이전 감사의 1268개 결과는 약 12.88ms였다. 같은 장비의 별도 실행 결과를 참고 비교한 것이며, 같은 시각의 전후 A/B나 종단간 지연 측정은 아니다.

현재 AU 1268개는 전부 보존된다. [캐시 정책](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Capture/CaptureSession+Setup.swift:81)과 [테스트](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Tests/RetransmitRingTests.swift:84)가 이 예외를 의도적으로 유지한다. AU 수·fragment 수와 함께 byte cap, 수명, 재전송 가치, 여러 세션의 총예산을 정의해야 한다. 무작정 512에서 자르면 큰 IDR의 복구율이 나빠질 수 있으므로 실제 IDR 크기 분포와 RTX hit율을 함께 봐야 한다.

### G. idle 전력과 큰 클립보드 이미지 반복 처리 — 우선순위 중간/낮음

현재 [getStatus 루프](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/src/use-stream-controller.ts:83)는 활성 스트림이 없어도 2초마다 동작한다. [이미지 클립보드](/Users/loopy/dev/ll3/leftcar/apps/viewer-expo/src/clipboard-sync.ts:279)는 PNG/base64를 읽고 전체 해시를 만든 뒤에야 중복을 판단한다. 정지한 큰 이미지도 poll마다 변환 비용을 낼 수 있다.

**개선:** 활성 스트림·화면 소유권에 맞춘 polling과 native 변경 revision 우선 확인을 검토한다. 먼저 Android의 wakeup, CPU, 전력 및 동일 이미지 반복 처리량을 측정한다. 활성 스트림 복구에 필요한 상태 확인을 끄는 방식은 적절하지 않다. scrcpy의 변화 기반 capture/최신 프레임 유지도 idle 비용을 평가할 비교 기준이지만, Leftcar 역시 최신 capture 슬롯을 이미 사용하고 있어 해당 구조의 부재를 지적하는 것은 아니다. [scrcpy 영상 동작](https://github.com/Genymobile/scrcpy/blob/19c1261d2e2cbf2b5e6a71a8b64cc1dd3ede06ac/doc/video.md#frame-rate)

### 당장 복사하지 않을 encoder 설정

Sunshine의 FFmpeg rate-control 버퍼와 Leftcar VideoToolbox DataRateLimits는 의미가 같지 않다. 더 작은 burst 예산은 화질·IDR 크기를 함께 측정할 실험이다. 특히 최신 Sunshine은 Apple H.264에서 참조 프레임 1개 강제가 모든 프레임을 IDR로 만드는 문제 때문에 이를 피하는 주석을 갖고 있다. Leftcar에 ‘참조 프레임을 무조건 1개로’ 같은 설정을 그대로 이식하지 않는 편이 좋다. [Sunshine VideoToolbox 설정](https://github.com/LizardByte/Sunshine/blob/0f7dcf3dc20b2dfee0912e07f422f812a42d1621/src/video.cpp#L1396-L1408)

## 실제 성능 비교의 다음 완료 기준

기존 [perf-matrix](/Users/loopy/dev/ll3/leftcar/tools/perf-matrix/README.md)를 확장하면 된다. 새 도구 체계를 처음부터 만들 필요는 없다.

| 축 | 최소 비교 조건 |
|---|---|
| 빌드 동일성 | Host commit·실행 파일 hash, APK·native lib hash, codec 이름·설정 |
| 기준선 | 현재 수정 빌드 vs 후보 변경 빌드. 같은 해상도·FPS·bitrate뿐 아니라 화면의 가독성/화질도 확인 |
| 장면 | 정적 문서, 텍스트 스크롤, 영상, 큰 화면 변화/IDR, 무음→오디오 재개 |
| 부하 | 단일 창, 2창, 4창; single/split; 1440p 기준선과 4K 후보 |
| 전송 | 동일 LAN/Wi-Fi와 USB. 통제된 jitter·loss·reorder, 재접속·resize·sleep/wake |
| 시간 | 각 조건 반복 단기 표본, 10분 안정성, 60분 발열·지연 누적 검사 |
| 영상 지표 | capture/encode/send/decode/display 구간별 p50/p95, 실제 화면 표시 간격, first frame, 복구 시간, frame-age, drop 원인 |
| 자원 지표 | CPU·RSS·할당·큐 byte·wakeup·온도·소모 전력, 디코더/인코더 인스턴스 |
| 사용감 | 입력→광자와 glass-to-glass 카메라 측정, 오디오 underrun·영상/음성 차이 |

RTT·시계 보정으로 계산한 frame-age, decoder Surface release 시간, 실제 화면에 빛이 나온 시간은 서로 다른 지표다. 후보 변경이 평균 FPS만 유지하면서 p95 지연·복구·발열을 악화시키면 승격하지 않아야 한다. Sunshine/Moonlight의 수치는 같은 장치에서 실제로 돌린 표본으로만 성능 순위를 매긴다.

## 권장 진행 순서

1. **정확성과 회귀:** F02 세션 종료/철회, F09 참조 안전성, F05/F06 호스트 수명, F11 영속 실패, F14 첫 배치 복구를 닫는다.
2. **검증 경로:** Android CI 의존성, Gradle 입력, split 테스트 fixture를 정리하고 모든 자동 gate의 실제 결과를 남긴다. F10/F20/F21 UI 경계도 함께 보호한다.
3. **작고 측정 가능한 성능 변경:** FEC 조기 반환, 오디오 버퍼 계측, RTX 총 byte 예산을 우선 실험한다.
4. **기기별 선택 최적화:** display pacing, decoder recipe·예약, Windows 송신, Opus·idle 전력을 같은 빌드 매트릭스로 평가한다.
5. **제품 완료 판정:** F27 정책 통합은 각 회귀 보호 후 점진적으로 진행하고, F28 배포와 F29 장시간 실기기 증거를 별도 완료한다.

이번 작업은 조사·검증만 수행했다. 저장소 소스·설정·문서를 수정하거나 커밋·설치·배포하지 않았으며, 새 보고서와 증거는 저장소 밖에 저장했다.
