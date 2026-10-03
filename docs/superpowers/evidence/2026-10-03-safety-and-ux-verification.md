# Leftcar 보안·동작 안정성·UX 검증 기록

이 기록은 후속 연결 안정성 수정 이전의 검증 스냅샷이다. 최종 소스의 테스트 수·품질 검사·해시 및 남은 검증 항목은 [최종 연결 안정성 자동 검증](2026-10-03-connection-reliability-verification.md)을 따른다. 아래의 과거 workspace fmt/Clippy 실패와 빌드 산출물 해시를 최종 소스의 결과로 해석하지 않는다.

대상은 현재 작업 트리의 Tauri Host, Expo Android Viewer와 네이티브 스트리밍 UI다. 기존 변경을 유지하면서 보안과 동작 안정성을 함께 보강하고 공통 디자인을 적용했다. 커밋, 게시, 설치, 데이터 초기화, 기기 권한 변경은 수행하지 않았다.

기준: [요구사항](../specs/2026-10-03-safety-and-ux-system-design.md), [실행 계획](../plans/2026-10-03-safety-and-ux-system.md). 이 기록은 코드와 자동 검증의 완료 범위를 설명하며, 취약점이 전혀 없거나 실기기에서 장시간 안정적으로 동작한다는 인증이 아니다.

## 요구사항별 근거

| ID | 적용한 동작과 확인 근거 |
|---|---|
| S1 | Host `identity.rs`는 기존 식별자 읽기·형식·버전·저장 오류에서 임시 키로 대체하지 않고 시작을 중단한다. 신규 파일은 `create_new`, 크기 제한과 Unix 비공개 권한을 사용한다. Viewer `session.ts`는 식별자 검증 후 자격 증명을 전송한다. 식별자·세션 회귀와 독립 소스 검토로 확인했다. Windows ACL의 실환경 검증은 포함하지 않는다. |
| S2 | Host 선택 변경과 화면 이탈이 이전 socket/catalog/trust 작업을 무효화한다. `session.ts`, `use-stream-controller.ts`, `stream-lifetime-operation.ts`는 세션 번호 재사용 시에도 port·startedAt·reservation 및 Host 선택을 확인한다. `host-lifetime`, `hub-connect`, `catalog-lifetime`의 실제 앱 훅 회귀가 통과했다. |
| S3 | Host 정책은 읽기 실패 시 쓰기를 잠그며 native ACK 후 표시를 바꾼다. Viewer 로컬 설정은 즉시 사용 의도를 반영하되 저장 중·미저장·재시도를 구분한다. `Privacy.tsx`, `FileShareCard.tsx`, `usePairingModel.ts`, `use-catalog-preferences.ts` 및 실제 실패/재마운트/ACK 순서 회귀로 확인했다. |
| S4 | Host AOAP 큐·쓰기와 outgoing 파일 admission을 제한했다. 인증 실패 IP 기록은 만료시키며 최대 1,024개로 제한하고 살아 있는 ban을 eviction하지 않는다. 준비 TCP drop의 full-queue 정지 교착을 제거했다. Android 물리 USB owner와 논리 media lease를 분리해 이전 lease가 새 세션을 취소하지 못하게 했다. TCP control 미완성 프레임 크기 제한·새 연결 초기화, writer/challenge 실패 시 owner 종료 상태, 재바인딩 경쟁 회귀가 통과했다. 붙어 있는 실제 accessory driver의 blocking read/write 종료는 실기기 미검증이다. |
| S5 | 파일 경로·이름·인증 credential 소유자·크기·동시 전송 제한, token 기반 staging 이름, 충돌 시 실제 저장 이름 ACK, rename 실패 후 취소 가능한 staging을 확인했다. 철회 후 idle을 기다려 해당 credential의 전송만 정리하며 동일 기기의 새 pairing 전송을 보존한다. 삭제 실패 staging은 소유권을 유지하고 다음 철회 재시도/admission/sweep에서 다시 정리한다. Viewer picker/전송은 최초 Host에 고정되며 취소된 staging을 정리한다. `file_transfer.rs`, `file-transfer.test.ts`, 실제 picker 중 Host 변경 회귀가 통과했다. |
| S6 | `tauri.conf.json`의 제한된 CSP를 실제 설정 값으로 브라우저에 적용해 script/network 정책을 확인했다. 외부 리소스와 불필요한 디자인 스타일을 제거했으며 Host production build가 통과했다. |
| S7 | Host 입력 요청·승인·철회·중단은 명시적이며 오류를 해당 행/창에서 표시한다. Android 승인 버튼은 현재 instance·generation·UUID requestId에만 결과를 전달한다. 요청 전송 성공은 Host 승인 대기 상태이고 입력 권한을 켜지 않는다. 실패/8초 timeout/Activity 재생성 회귀와 JVM 검증이 통과했다. |
| D1 | `packages/ui-tokens/src/tokens.ts`가 색·글자·간격·반경·최소 영역을 소유한다. `theme-css.ts`, `theme-kotlin.ts`, `generate-ui-theme.ts --check`가 CSS/Kotlin의 일치를 검증한다. |
| D2 | 공유 `recipes.ts`는 실제 `cva`를 사용하며 `cn.ts`는 의미 있는 글자 크기와 색을 함께 유지한다. 대비/클래스 병합 테스트 25개와 실제 컴파일 결과를 확인했다. |
| D3 | Host DOM primitives는 Tailwind, Viewer RN primitives는 Uniwind를 사용한다. Vite CSS, Metro Android export 외에도 설치된 Uniwind의 production `compileCSS`를 Android 플랫폼으로 실행했다. 두 테마의 실제 native color·48/44px 영역·15/12px 글자·2px focus outline이 확인됐다. |
| D4 | Host dashboard·session·pairing·settings·modal·diagnostics와 Viewer hub·host·pairing·catalog·cards·proof 화면을 공통 역할로 옮겼다. 중복 CSS/StyleSheet/옛 variants를 제거했다. SafeArea/Camera 같은 제3자 RN 컴포넌트는 `withUniwind`로 연결했다. 런타임 inset·영상 비율·진행률·StatusBar 및 native video geometry만 플랫폼 동적 스타일을 유지한다. |
| U1 | 권한 확인 전에는 거부 상태를 만들지 않는다. 첫 화면은 권한·연결·페어링의 실제 다음 동작을 제시하며 LAN을 WAN 가용성으로 표시하지 않는다. Host/Hub/Host-picker 실제 상태 회귀로 확인했다. |
| U2 | Catalog는 화면 선택과 현재 스트림을 먼저 보여 주며 고급 품질·UDP·실험은 하나의 설정 진입점으로 모았다. 다중 창, 스트림별 크기/소스 변경과 중단을 유지했다. 소스 전환 중 해당 스트림에 진행 상태를 표시한다. |
| U3 | 중단·철회·파일·설정·입력 승인에서 진행/지역 오류/재시도를 제공하고 잘못된 실험 입력을 보존한다. 없는 telemetry는 측정 중으로 표시한다. 설정 sheet 안에 native 전달 진행·오류·Retry를 표시한다. 재시도는 실패한 원래 값과 화면에 고정되며 Host/창/예약 generation 변경 시 취소된다. owner epoch로 cleanup/setup 후 이전 결과를 막고, 나중에 선택한 UDP 설정의 dirty 경고를 이전 재시도가 지우지 못하게 했다. 실제 오류/동일 값 Retry 브라우저 회귀와 controller 수명 회귀가 통과했다. 실제 미디어 설정 적용은 기기 미검증이다. |
| U4 | 글자 최소 12px, 정상 텍스트 대비 4.5:1, 조작 영역 최소 44 논리 px, busy/disabled/selected 의미와 focus를 확인했다. Native HUD는 불투명 단색·공통 토큰·명시적 입력 승인 버튼·접근성 timeout을 사용한다. 실기기 TalkBack/하드웨어 키보드 수용성은 미검증이다. |
| U5 | PIN 숫자 필터·붙여넣기·포커스·잘못된 코드 오류와 QR 권한 거부/설정 복귀/blur/모드 전환/unmount 카메라 해제를 실제 컴포넌트 회귀로 확인했다. |
| U6 | Host 780×540, Viewer 320px와 1024×600, 한국어/영어 및 밝은/어두운 테마에서 overflow·영역·dialog·keyboard를 확인했다. 목적을 정한 Host first-run/Viewer catalog 화면만 캡처해 직접 확인했다. native 영상 화면은 캡처하지 않았다. |
| V1 | 마지막 Viewer sheet/generation/UDP revision 변경 후 Root React Doctor 100/100, 타입 및 관련 테스트가 통과했다. findings 숨김/점수 override는 사용하지 않았다. |
| V2 | 독립 Host/Viewer/native 검토와 현재 소스, 실제 앱 훅·컴파일·build·JVM 결과를 대조했다. 마지막 수정까지 독립 검토 CLEAR이며 아래 자동 검증과 실기기 경계를 구분했다. |

## 실행 결과

- React Doctor: root `npx -y react-doctor@latest . --verbose`, **100/100**, 183 파일, findings 없음. 최종 진단: `/var/folders/z8/h16kj6d16t53dj0lfvlkxf0h0000gn/T/react-doctor-ca35ccc2-d85f-4dc6-8130-199e0f8652e5`.
- Root `bun run typecheck`: 통과. generated CSS/Kotlin drift, 정확한 React runtime 버전과 두 앱 tsc 포함. `/tmp/leftcar-final-typecheck.log`.
- Root `bun run test`: **75 파일, 923 테스트 통과**. `/tmp/leftcar-final-js-tests.log`.
- `bun run test:contract`: **4 통과**. `bun run test:architecture`: TS/Kotlin rules clean. `git diff --check`: 통과.
- `bun run test:ui`: **14 suites, 103 PASS, exit 0**. 실제 앱 동작을 유지하고 OS/transport 경계만 제어한 브라우저 회귀 13 suites와 실제 Uniwind native compilation 포함. `/tmp/leftcar-final-ui-tests.log`.
- Host frontend production build: **통과**, Vite 1915 modules. `/tmp/leftcar-final-host-build.log`.
- Android production Metro/Hermes export: **통과**. `/tmp/leftcar-ux-final-export-20261003`, `/tmp/leftcar-final-android-export.log`.
- Rust `cargo test --workspace --locked`: **461 통과, 0 실패, 2 ignored**. ignored는 기존 vector printer tests다. Android Viewer 283 tests 포함.
- Host Rust `--lib` suite: **333 통과, 0 실패**. Host Clippy `-D warnings`, Host fmt 및 수정 Rust 파일 fmt: 통과. `host-tests-final.log`, `host-clippy-final.log`, `host-fmt-final.log`, `changed-files-fmt-final.log`.
- Host native debug `cargo build --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked`: **통과**. `/private/tmp/leftcar-safety-native-20261003/host-build-final.log`.
- Android aarch64 release `.so`: 현재 native 소스 새 컴파일 통과. `target/aarch64-linux-android/release/libleftcar_viewer.so`, 1,468,656 bytes, SHA256 `4dadad9bb3b4d21464cacac65ad57b367cc106d7677ab861c52bd37f8939527c`.
- Native Rust receipts와 source hashes: `/private/tmp/leftcar-safety-native-20261003/README-final.md`, `counts-and-artifact-final.json`, `native-source-hashes-final.json`. 최종 Host-only 수정 이후 Android/workspace native hashes는 동일하다.
- Android `./gradlew :app:testDebugUnitTest --console=plain`: **29 suites, 158 tests, 실패/오류/skip 0**, Kotlin main compile 포함. 실제 설정 ReactMethod 5종의 실행 예외 전달도 확인했다. `/private/tmp/leftcar-viewer-android-jvm.log`, `apps/viewer-expo/android/app/build/test-results/testDebugUnitTest/` XML.
- 최종 소스 해시: `/tmp/leftcar-final-source-hashes.json`. 문서 외 수정·신규 app/package/native/tool 파일의 해시와 base commit을 기록한다.

## 검증 경계

브라우저 회귀는 실제 앱 컴포넌트와 상태를 실행하지만 Android OS를 대체한다. Uniwind native compiler 검증은 RN style table을 확인하지만 물리적 렌더링을 증명하지 않는다. Robolectric/JVM과 aarch64 `.so` 빌드도 기기 영상·입력·권한 동작을 증명하지 않는다.

ADB 읽기에서 TB710FU 기기 1대의 연결은 확인했다. 이 후보는 설치하지 않았으므로 연결 확인을 후보 실행 증거로 사용하지 않는다. 실제 USB driver unplug/blocked-write 해제, 장시간 전송·다중 영상 창·macOS 권한·Windows 실행 및 ACL, TalkBack/하드웨어 키보드는 이 실행에서 검증하지 않았다. 설치 가능한 APK/서명 앱 배포와 기기 설치도 수행하지 않았다.

전체 workspace fmt는 변경하지 않은 `keymap`, `viewer-decoder` 파일의 기존 형식 차이 때문에 통과하지 않는다. 전체 workspace Clippy `-D warnings`는 변경하지 않은 `secure-channel` 테스트의 기존 `unused_mut` 3건 때문에 실패한다. Android Viewer Clippy `-D warnings`는 변경하지 않은 `prepared_udp.rs:192`의 기존 `type_complexity` 때문에 실패한다. Android release build에도 기존 미사용 control-response helper warning이 있다. 이 결과를 숨기거나 관련 없는 소스를 수정하지 않았다. 변경 영역의 fmt와 Host Clippy는 통과했다.
