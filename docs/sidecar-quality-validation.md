# Sidecar 품질 개선 검증 기록

작성일: 2026-09-05. 최종 갱신: 2026-09-06. 상태: 코드·자동 검사 완료, 실제 Lenovo USB/ADB 연결 진단 중. 일부 화면 실기 검증 차단 및 주 화면 전환 현상 미해결.
요구사항: 반응 속도 최우선, 읽기 쉬운 글자, 연속적인 영상, HiDPI 가상 화면 추가·배치, Android/XR 창 비율.

## 환경

- Mac: M1 Max, macOS 26.6.2(Darwin 25.6.0), 외장 DP 3840×2160 / 논리1920×1080 / 60Hz.
- BetterDisplay 실행 확인. 기존 디스플레이를 수정하지 않고 작업에서 만든 가상 화면만 시험한다.
- Android: USB ADB와 무선 mDNS 검색 모두 비어 있음. 실제 기기 모델·OS·연결 방식 미확정.
- 기반 커밋: 47236ce. 개선 작업 브랜치: codex/sidecar-quality-display.

## 개선 전 인코더 기준선

기반 커밋의 소스를 별도 임시 디렉터리에 추출해 빌드했다. 작업 중 파일 수정이 측정 바이너리에 섞이지 않는다. 실제 하드웨어 접근이 가능한 환경에서 동일한 12초 moving 합성 패턴을 순차 실행했다.

| 경로 | 제출 프레임 | 유효 프레임 | 유효 FPS | 드롭 | 콜백 p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| single RTVC (타일 프로브) | 720 | 629 | 52.44 | 91 | 10.41ms |
| dual AVE 왼쪽 | 720 | 720 | 59.88 | 0 | 38.03ms |
| dual AVE 오른쪽 | 720 | 720 | 59.88 | 0 | 37.80ms |

실행: `baseline-throughput-probe single 12 moving`, 이어서 `baseline-throughput-probe dualAve 12 moving`.
도구 소스: tools/codec-probe/TileEncoderThroughputProbe.swift.

해석: AVE 조합이 이 부하에서 더 연속적인 출력을 내지만 콜백 지연도 크다. 이 수치는 타일 인코더 프로브이며 전체 화면 캡처·전송·디코더·디스플레이 지연 또는 실제 영상 시청 결과가 아니다. 전체 스트리밍 4K60 달성 근거로 사용할 수 없다.

## 검증 진행 상황

- 개선 전 루트 타입 검사·TS 테스트 및 Rust workspace 테스트 통과. TS 346개에는 과거 작업 스냅샷149개가 포함된다.
- 전송 변경 1차 독립 리뷰에서 실제 수락 해상도의 상태 표시 불일치와 bitrate-floor 이벤트 연동 누락을 발견해 수정 요청했다.
- Android 변경 1차 Gradle 검사에서 발견한 세로 비율 기대값 오류를 수정했고 후속 JVM 검사는 통과했다. 커서 패킷 재전송 변경 뒤 최종 패키지는 다시 검증한다.
- 변경 후 최종 React Doctor·타입·각 플랫폼 검사 및 패키지 검증은 아직 완료하지 않았다.

## 완료에 필요한 실기 증거

- Android 연결 및 정확한 빌드 설치.
- 정지 글자·스크롤·창 이동·영상의 동일 조건 전후 비교.
- 실제 rendered FPS·프레임 간격·capture age·queue age·decoder/output drops·복구 기록.
- HiDPI 논리 크기와 backing 픽셀 검증, 개별 생성/이동/제거와 반복 정리.
- Android freeform 초기 비율, 사용자 크기 변경 뒤 재연결, 입력 좌표 일치.
- Galaxy XR 공간 창 조작과 글자 가독성은 해당 기기에서 검증. 일반 태블릿 결과로 대체하지 않는다.
- 180초·10분 관찰과 기존 E7 기준의60분 soak. 자동화 테스트로 장시간 실기 입증을 대체하지 않는다.

## BetterDisplay 실제 HiDPI 계약 실험

고유 임시 이름으로 각각 생성→연결→2초 대기→조회→해당 이름만 제거했다.

- 잘못된 후보: aspect1600×1000, multiplierMin/Max3200×2000, HiDPIon. 조회가 `on,3200x2000,on`으로 나와 작업 공간이 의도보다 두 배다.
- 수정 후보: aspect1600×1000, multiplierMin/Max1600×1000, HiDPIon. 조회 `on,1600x1000,on`; system_profiler가 **Resolution3200×2000 / UI Looks like1600×1000 @60Hz**를 확인했다.
- 올바른 모드가 이미 선택된 상태에서 동일 모드 set이 Failed를 반환한 표본이 있어, 모드 확인 없이 set 종료 코드만으로 판정하면 안 된다.
- CLI help는 `name`을 exact displayed-name selector로 문서화한다. 존재하지 않는 고유 이름 조회는 Failed, 기존 DP 조회는 on,1920x1080이었다. 다만 중복 이름은 여러 대상을 가리킬 수 있으므로 제품 소유권은 고유 tagID/UUID까지 확인해야 한다.
- 생성된 가상 화면이 main으로 올라간 표본이 있었다. 원래 anchor를 생성 전에 확보하고 실제 배치를 검증해야 한다. 제거 후 임시 화면은 사라지고 기존 출력은3840×2160/논리1920×1080으로 확인되었다.

이는 CLI·macOS 모드 검증이며 Host 앱 경유 생성/스트리밍 및 Android에서의 가독성 검증을 대신하지 않는다.

## CGVD 실제 생성·수명 검증

release shim으로 고유 QA 이름/serial을 사용하고 stdin을 유지했다.

- scale1: `READY 10 1600 1000 1600 1000`, 프로세스 유지 중 해당 화면 존재.
- scale2: `READY 11 1600 1000 3200 2000`, macOS가 논리1600×1000/3200×2000/60Hz 확인.
- 각 세션에 stop 전달 후 종료 코드0, 후속 화면 목록에서 두 CGVD 화면 모두 제거 확인.
- 독립 BetterDisplay QA 화면이 함께 존재했으므로 이 측정은 HiDPI와 세션 수명 증거로만 사용한다. 여러 모니터 배치·주 화면 보존 성공으로 확대하지 않는다.
- Rust 세션이 오류 경로에서 drop돼도 child를 종료·회수하도록 보강했다. 이미 종료된 child 제거와 drop 시 잔존 여부 테스트를 포함한 Host96개 단위/10개 통합 테스트가 통과했다.

## 통합 검토 후속 사항

- React 복잡도 우회 wrapper를 실제 기능별 컴포넌트로 분리했다. 담당자 실행 React Doctor100/100 확인; 최종 전체 코드가 안정된 후 루트에서 재검사한다.
- 커서 상태 UDP 유실 후 재전송 부재와 설정FPS를 실제 처리량처럼 사용한 해상도 복귀 조건을 독립 리뷰에서 발견했다. 수정·재검증 진행 중.
- 설치된 Host에서 CGVD 실행 파일을 찾는 리소스 경로와 배포 포함 여부를 보강 중이다.
- 가상 화면 오류 시 정리가 실패해도 소유권을 잃지 않도록 오류 경로를 추가 검토 중이다.
- Android USB/mDNS를 재확인했으나 여전히 장치가 없다. 설치·실시간 영상·Galaxy XR 창 조작은 미검증이다.

## 최종 Android 설치본과 공통 검사

프레임 재정렬·세로 인코더 정책·커서 상태 갱신 후 검사:

- 루트 React Doctor **100/100**, 86파일 검사, 이슈 없음.
- 루트 타입 검사 및 TS30파일355테스트 통과(과거 스냅샷149 포함, 현재 제품206).
- 계약 테스트4개와 TS/Kotlin 아키텍처 검사 통과.
- Rust workspace 테스트·Clippy `-D warnings`·포맷 검사 통과.
- Android Viewer125개 단위 테스트 및 ARM64 대상 검사 통과.
- JavaScript를 포함하는 release APK 빌드·Android JVM 테스트 통과. XR 플랫폼 API는 공식 지침의 compileOnly 의존성을 사용했고 R8을 유지했다.
- APK: `apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk`
- APK SHA-256: `90fe5b48e59b08e6512b26e419c7cb2b34a0758d1ce574d3bad38b0a8e5976b4`
- 빌드된 native SO와 APK 병합 전 SO가 동일하다. APK 내부 SO는 빌드 도구의 strip 이후 SO와 동일하다.
- APK에 `assets/index.android.bundle`(3161872바이트)과 ARM64 viewer SO가 포함된다. 개발 서버 없이 실행 가능한 테스트용 서명 빌드다.
- USB 장치 목록에서도 Samsung/Android 관련 장치가 발견되지 않았다. 설치·실행·영상 실측은 아직 수행하지 못했다.

## CGVD 관리자 경로 실기와 후속 발견

- 관리자 경로로 일반/HiDPI1600×1000 화면 두 개를 생성하고 비중첩 배치, 첫 화면 아래로 이동, 전체 정리를 수행한 opt-in 테스트가 통과했다.
- 생성 전에 실행 중인 프로세스는 새 CGVD의 active/online/bounds를 보지만 current mode는 nil이었다. 생성 이후 시작한 새 프로세스는 정상 모드를 읽었다. 부모의 run loop를 처리해도 해결되지 않았다.
- 생성·보유 중인 shim 프로세스가 PLACE를 수행하고 PLACED 좌표 응답을 검증하는 방식으로 배치 문제를 해결했다.
- 활성 배치 중 primary ID 유지와 제거 전후 UUID 일치가 통과한 표본이 있었지만, 이후 최신 메타데이터 ABI 적용 실기에서는 제거 뒤 UUID가 바뀌었다. 따라서 제거 전후 주 화면 유지가 항상 보장된다고 결론내리지 않는다.
- 동일 부모 프로세스의 HiDPI 조회 실험에서 current mode/all modes/NSScreen 모두 비었고 SCDisplay 크기도1600×1000이었다. 따라서 캡처 목록의 fallback만으로는3200×2000을 전달할 수 없다. 소유 엔진이 확인한 실제 mode를 nativePixelSize에 연결했다. 최신 UUID/generation ABI 실기에서 raw mode=nil인데도 캡처 목록3200×2000을 확인했고 해당 화면과 메타데이터를 정리했다.
- 기본 BetterDisplay 관리자 경로도 같은 프로세스의 모드 캐시 문제와 생성 시 primary 변경을 확인했다. 빠른 독립 mode 조회·주 화면 복원까지 실기 검증 중이며 아직 완료로 표시하지 않는다.

## 최종 미해결 및 승인 경계 (2026-09-06)

1. 실제 테스트 대상은 사용자가 확인한 Lenovo 태블릿이다. USB 장치와 ADB 인터페이스는 존재하지만 native ADB에서도 offline이며, 기본 libusb 백엔드에서는 목록에 나타나지 않았다. 앞선 장치 부재 판단은 아래 USB 정정 기록으로 대체한다. 설치본은 준비했지만 실제 연결/입력/영상/글자/공간 창/장시간 테스트는 수행하지 못했다.
2. 최신 CGVD 두 화면 실기에서는 생성·이동·비중첩·개별 수명 정리가 동작했으나, 미러링된 물리 화면의 primary UUID가 제거 후 다른 물리 화면 UUID로 전환됐다. 생성 화면은 정리됐지만 이 현상의 영향과 해결은 미검증이다.
3. BetterDisplay 관리자 경로 최종 재시험은 자동 승인 검토에서 거부됐다. 사유는 반복된 실패, BetterDisplay 무응답, 이전 화면 정리 시간 초과로 인한 지속적인 화면 설정 영향 위험이다. 이후 추가 생성·재실행으로 우회하지 않았다.
4. BetterDisplay를 재시작한 이후 읽기 전용 화면 목록에서 이전 두 QA 이름은 보이지 않았지만, CLI가 응답하지 않아 UUID를 이용한 명확한 제거 확인은 남아 있다. 소유권 정보가 남으면 앱의 정리 재시도로 회복하도록 구현했다.
5. 최신 Mac 앱은 별도로 패키징하며, 실행 중인 설치 앱을 교체·재시작하거나 추가 가상 화면을 생성하지 않는다. 최종 패키지 경로와 리소스 검증 결과는 아래에 기록한다.

승인 검토가 거부한 명령은 기본 BetterDisplay를 이용해 두 개의 실제 가상 화면을 생성·배치·제거하는 opt-in 테스트였다. 구체적인 수정·자동 검사 결과는 준비되어 있으나, 이 실기 작업의 재개에는 추가 승인이 필요하다. 커밋·푸시는 아직 수행하지 않았다.

## 2026-09-06 실제 USB 장치 재진단 정정

이전 ‘USB 장치 없음’ 판단은 충분하지 않았다. macOS26에서는 `SPUSBHostDataType`을 사용해야 하는데 구형 `SPUSBDataType` 조회와 제조사 이름 검색에 의존했다. IORegistry 직접 확인 결과 Lenovo VID0x17ef/PID0x7f58 장치가 이름 없이 연결돼 있으며, USB interface class255/subclass66/protocol1(ADB)을 제공한다.

- 기존 ADB36.0.2 LIBUSB 서버는 빈 장치 목록을 반환한다.
- 별도5038포트의 NATIVE backend는 같은 실제 USB 장치를 `(no serial number) offline`으로 인식했다.
- 해당 ADB 인터페이스의 점유 주체는 진단 ADB로 확인했다.
- offline 재협상 후에도 응답이 없었다. 동일 VID/PID로 유일하게 확인한 기기의 USB 연결 reset 요청은 libusb timeout(-7)이었다. 데이터 삭제·Android 재부팅은 하지 않았다.
- 사용자에게 물리 케이블 재연결과 잠금 해제를 요청했다. 에뮬레이터나 가상 Android로 실기 증거를 대체하지 않는다.
- Host 첫 화면 확인에서 테스트 중 켰던 실험 설정이 남아 기존 화면 확장 카드와 신규 관리 카드가 함께 노출됨을 확인했다. 설정을 원래 off로 복원하고 AX에서 카드가 숨겨진 것을 확인했다. 실제 Mac 주 모니터·배치는 이 조치로 수정하지 않았다.

## 최종 Mac 패키지

- 앱: `apps/host-desktop/src-tauri/target/release/bundle/macos/Leftcar Host.app`
- 최신 capture dylib/CGVD shim과 번들 리소스의 바이트 일치, 실행 파일 및 앱 서명 검증 통과.
- 추가 화면 변경을 피하기 위해 최종 번들을 실행하거나 `/Applications` 설치본을 교체하지 않았다.
- libleftcar_capture.dylib SHA-256: `724083dee03d76301d51d619af0e97b6bc7f7d28f345bb9731fb514d112fa2ce`
- cgvd-shim SHA-256: `786330987c317f58b2f47e05974179338e44ca6c6a7ac1e1f7c3a5f6b2bf17f4`

## 2026-09-06 재연결 및 첫 화면 정리 후속

- 사용자가 실제 테스트 대상을 Lenovo 태블릿으로 확인했고 케이블 재연결과 잠금 해제를 완료했다. IORegistry USB sessionID가 3650558899731에서 3667546516214로 변경돼 해당 Lenovo 장치의 재열거를 확인했다.
- ADB 서버를 하나로 정리하고 기본 5037 포트에서 native USB 백엔드로 재시작했다. 장치는 여전히 `(no serial number) offline`이다. 에뮬레이터로 대체하지 않았으며 태블릿의 USB 디버깅 재활성화를 요청했다.
- 첫 화면의 중복 가상 화면 카드는 단일 접이식 항목으로 통합 중이다. 실험 설정은 설치된 기존 앱에서 꺼짐으로 복원했다. 새 UI의 빌드와 React Doctor는 통과했으며 루트 검토에서 발견한 고급 세션 시작의 opt-in 경계와 소스 가독성을 추가 수정한다.

- 첫 화면 후속 수정 완료: 실험 설정이 꺼져 있으면 고급 세션 시작도 숨김 처리했고 접기/펼치기 표시를 추가했다. 최종 React Doctor100/100 이후 루트 타입 검사, 355개 테스트, 계약4개 및 구조 검사 통과. 새 UI를 포함한 Mac 앱 패키징과 strict/deep 서명 검증 통과. 설치·재실행은 수행하지 않았다.

## 2026-09-06 Lenovo 무선 실기 연결 및 스트리밍

- 사용자 제공 무선 디버깅 주소로 ADB 연결 성공. `getprop` 실측 모델 TB710FU, 물리 화면2000×3200. USB 경로는 여전히 offline이므로 USB 전송 성공으로 해석하지 않는다.
- 최신 APK 설치 성공, 기존 저장된 페어링으로192.168.0.134:7777 Host 연결 성공. 활성 세션/관리 화면 복구 목록이 비어 있음을 확인한 뒤 최신 Host를 `/Applications`에 설치하고 서명을 검증했다. 가상 화면 생성·배치 재시험은 수행하지 않았다.
- 실제1440p 화면 수신 및 H.264 저지연 디코더 실행, 종횡비를 유지한3200×1800 표시와 위아래 여백을 스크린샷으로 확인했다. 입력이 꺼진 세션이므로 원격 입력 지연 성공 증거는 아니다.
- 1440p 수집기 첫 실행은 sandbox가 Mac unified log 접근을 막아 실패했다. 별도의 읽기 전용 승인으로 수집을 재개했다. 최초 요약은 Host/Android 구간이 달라 비교 근거에서 제외했다.
- 정확히 겹친09:14:02.036–09:16:07.130(125.094초)의1440p 로그: Host capture7360/encode6943, encode FPS 샘플 평균55.39. Android decoder inputDrops0, outputDrops증가206, frameGaps증가4. outputDrops는 최신 화면 유지용 출력 건너뜀이며 직접적인 네트워크 손실량으로 해석하지 않는다. 주요 병목 후보는 Host 인코더 출력 주기다. captureAge는 input-to-photon 측정이 아니다.
- 선명한 화면4K 설정은 split 두 인코더·디코더 경로를 선택했다. 실제 Mac에서30초60fps 영상 패턴을 반복 재생했다. 빌드 부하 전09:18:42.961–09:19:29.041(46.08초) left/right rendered·joined FPS 중앙값59/60, joined증가2745, inputDrops/incomplete/unmatched0, left gap1. 짧은 구간이므로 4K60 장시간 합격으로 기록하지 않는다. pairTimeouts는1ms 대기 만료 카운터이고 실제 pair손실(unmatched)과 다르다.
- 뒤로 가기 정상 종료를 Host 오류/Viewer 컴퓨터 종료 알림으로 잘못 표시하는 실제 문제를 발견해 수정했다. 관련 회귀 테스트 및 React Doctor100/100 후 루트 타입 검사358개 테스트·계약4개·구조 검사 통과.
- 종료 안내 수정 APK SHA256: b9eeedc3f775d3548dfd2fd1d7cf8424e81ffdbcbe57c44a1921fcfdbace4fbd. 실기기 설치 성공. Mac도 수정본 패키징/설치/서명 검증 완료. split 경로 Back 이후 feedback timeout은 별도 조사 중이다.
- 로그/분석: `/tmp/leftcar-sidecar-work/lenovo-overlap-diagnosis.md`, `lenovo-current.host.ndjson`, `lenovo-clarity.android.log`. 장시간 영상, 동일 조건 전후 비교, 원격 입력, Galaxy XR 공간 창 및 가상 화면 teardown 문제는 남아 있다.
- 수정 APK의 추가 실기 검사에서 Catalog 진입 후 `TypeError: undefined is not a function`으로 종료되는 회귀를 발견했다. `stream-termination.native.ts`가 일반 파일을 대체하지만 새 분류 함수를 export하지 않은 원인이다. 타입·일반 테스트 통과와 Android release 실행은 별도 검증이 필요함을 확인했다. native entrypoint 회귀를 수정·검증 중이며 해당 APK를 최종 성공본으로 취급하지 않는다.
- Android native entrypoint export 누락 수정 후 플랫폼 엔트리 회귀 테스트 추가. React Doctor100/100 이후 루트 타입 검사 및31파일359테스트 통과. APK 재설치 후 실제1440p 스트림이 지속 렌더링했고 뒤로 가기 종료 때 Viewer는 잘못된 경고 없이 카탈로그로 돌아갔다. Host AX에서도 `연결된 기기에서 화면 공유를 종료했습니다`를 확인했다. 이 재검증으로 앞선 수정 APK 충돌 회귀를 해소했다.

## 2026-09-06 분할 4K BYE 최종 실기 검증

- 변경: `native/android-viewer/src/renderer/split_session/tile_worker.rs`가 공유 stop 플래그를 확인하기 전에 마지막 `Stop` 명령을 drain하도록 수정했다. 기존에는 coordinator가 stop을 세운 뒤 명령을 enqueue해 worker가 BYE를 보내기 전에 loop를 빠져나갔다. `StreamActivity`도 최종 Activity 종료의 Surface 소실 시 native release를 먼저 수행하도록 보강했다.
- 빌드: ARM64 native release build 성공, Gradle `assembleRelease` 성공. APK SHA-256: `f458e7c3fc9bbd84e860684168539d67a3d2440d9c02dbc78ffc8b6eb5a3c42d`. APK 내부 `lib/arm64-v8a/libleftcar_viewer.so` 포함 확인.
- 실기: 2026-09-06 09:42 KST, Lenovo TB710FU, ADB `192.168.0.19:34679`에 `-s` 지정해 APK `-r` 설치. `선명한 화면, 4K 60fps` 선택 후 split left/right decoder가 `1920x2160` 타일을 수신하고 left/right/joined 약 58–60 FPS, `unmatched=0` 표본을 기록했다. 이는 짧은 표본이며 성능 완료 주장이 아니다.
- 종료: Android logcat에서 `split shutdown requested sendBye=true terminationReason=-1` 및 `split Left sent stream close signal ... authenticated=true`를 정확히 1회 확인했다. Back 후 화면 선택 카탈로그로 복귀했고 Viewer 오류 대화상자/충돌은 없었다. `feedback timeout` 로그는 없었다.
- Host의 공식 읽기 전용 로그에서 이번 실행의 텍스트 `viewer closed stream`을 별도 추출하지 못했으므로 Host 표시 문구 자체는 미검증으로 남긴다. 다만 BYE는 인증된 Host peer(`192.168.0.134:51506`)로 전송됐다.
- 남은 한계: Host 문구의 독립 확인, 장시간 4K 성능(180초·10분·60분), 동일 조건 전후 품질 비교, 입력 지연, XR/가상 디스플레이는 미검증. 사용자 문서 `docs/tablet-cursor-streaming-validation.md`는 수정하지 않았다.

## 2026-09-06 분할 4K BYE 검증 재확인 (verification stage)

- 실기 시각: 2026-09-06 09:45–09:46 KST. 대상: 물리 Lenovo TB710FU, ADB `192.168.0.19:34679` 지정(에뮬레이터·USB offline 장치 미사용).
- 설치: release APK `-r` 설치 성공. APK SHA-256은 `f458e7c3fc9bbd84e860684168539d67a3d2440d9c02dbc78ffc8b6eb5a3c42d`; `lib/arm64-v8a/libleftcar_viewer.so`와 `assets/index.android.bundle` 포함을 확인했다.
- 수신: 기존 `192.168.0.134:7777` 페어링으로 화면 선택에 진입해 `선명한 화면, 4K 60fps`와 Display 0을 선택했다. native 로그에서 양쪽 `1920x2160` 하드웨어 디코더 준비, split left/right rendered/joined 약 59–60 FPS, `gaps=0 inputDrops=0 incomplete=0 unmatched=0`을 확인했다. 짧은 표본이므로 장시간 4K 성능 합격으로 확대하지 않는다.
- 종료: UI bounds 기반 Back 후 카탈로그의 화면 선택으로 복귀했다. `split shutdown requested sendBye=true terminationReason=-1` 1회와 `split Left sent stream close signal ... authenticated=true` 1회를 확인했고, `feedback timeout`, `FATAL EXCEPTION`, Viewer 오류 대화상자는 확인되지 않았다. `surfaceDestroyed` 뒤 final native release 로그도 확인했다.
- Host 공식 읽기 전용 로그에서는 이번 09:45 실행에 대응하는 `viewer closed stream` 문구를 별도 추출하지 못했다. 따라서 Host 표시 문구 자체는 여전히 미검증이며, 인증된 left BYE 전송과 Android 정상 복귀까지만 증거로 기록한다.
- 자동 검증: `cargo test -p android-viewer` 127개 통과. 기존 release native/Gradle 빌드 로그 성공을 재확인했다.
- 남은 한계: Host 문구 독립 확인, 180초·10분·60분 soak, 동일 조건 전후 품질 비교, 입력 지연, XR/가상 디스플레이는 미검증. 사용자 문서 `docs/tablet-cursor-streaming-validation.md`는 수정하지 않았다.
