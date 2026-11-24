# Host 4K 혼잡 붕괴로 인한 세션 종료 진단

- 작성일: 2026-11-24
- 범위: macOS Host 캡처 shim(`native/macos-capture-shim/`)의 UDP 페이싱·ABR·제어 평면. 뷰어 종료 사유 코드는 `native/android-viewer/src/renderer/single_session/health.rs`와 `runtime/worker.rs`만 참조한다.
- 목적: Host에서 유튜브 등 고운동 콘텐츠를 틀면 스트림이 "점점 느려짐 → 멈춤 → 검은 화면 → 재연결"을 반복하고, **4K일 때 훨씬 심한** 현상의 원인을 코드 경로로 확정하고 수정한다.

## 요약

원인은 IP·멀티홈이 아니라 **혼잡 붕괴(congestion collapse)** 다. 고운동 콘텐츠는 두 가지를 동시에 일으킨다.

1. **수요 급증**: dirty region ≥20% 또는 AU 압력이 0.8~2 s 유지되면 `video` 모드로 전환되고(`AdaptiveMotionPolicy.swift`) ABR이 대기 없이 motionFloor(1080p ≥14 Mbps, 4K ≥48 Mbps)까지 올린다. 4K video 모드 바닥이 24 Mbps라 그 아래로 잘 내려가지 않는다.
2. **페이싱 × 소켓 버퍼**: 로컬 LAN 피어는 델타도 2× 배율로 밀어내고(`udpPacingRateMultiplier`) 프레임 예산이 1.25×라 유효 페이싱은 약 2.5× wire rate(4K 48 Mbps → ≈150 Mbps). 소켓 송신 버퍼는 64 KiB로 의도적으로 좁아(`CaptureSession+Setup.swift` `SO_SNDBUF`) **링크 드레인 ≈65 Mbps 이하에서 4K AU(≈100 KB) 하나를 보낼 때마다 EAGAIN → AU 폐기 → IDR 요청**이 난다. 평균 48 Mbps를 감당하는 링크에서도 발생한다.

혼잡에 대한 반응이 느려 붕괴가 자가 유지된다.

- ABR은 1 Hz, 컷 확정에 2연속 윈도우, 컷 폭 −20%. 48 → ~5 Mbps까지 10 s 이상.
- 복구 IDR은 16~64 Mbps 버스트라 링크를 다시 막고, "recovery storm"이 된다(팀 주석 `UdpPacingPolicy.swift`: 578 recovery keyframes/hour, 0fps windows).
- 컷 직후의 고운동 분기는 **첫 깨끗한 윈도우(복구 grace 1 s)에 즉시 90% 천장까지 올라** 톱니(cut → raise → collapse)를 만든다.
- 결정적으로 프로브 응답(LCP2)·ACK이 **미디어와 같은 소켓**의 논블로킹 `sendto`로 나가고 결과를 버려서, 버스트 중엔 EAGAIN으로 조용히 유실된다. 뷰어는 프로브 응답 3연속 미수신(1 s 간격)만으로 `TerminateHostUnreachable`(reason 4)로 자체 종료 → 검은 화면 → 재시작(reason 1/4/5 모두 `stream-termination-policy.ts`에서 복구 대상).

"4K일 때 심하다"는 사용자 관찰은 ① motionFloor 24→48 Mbps, ② AU 크기(데이터그램 70개+)에 따른 페이싱 버퍼 overflow, ③ 손실 시 AU 단위 손실 확률 세 가지 모두와 일치한다.

## 수정 (이번 변경)

| # | 파일 | 변경 | 대상 증상 |
|---|---|---|---|
| 1 | `CaptureSession+ViewerControl.swift`, `CaptureSession+Setup.swift`, `CaptureTransportPolicy.swift` | 제어 평면 `sendto`가 EAGAIN이면 inputQueue에서 3→192 ms 배수 백오프(≈380 ms) 재시도. 프로브 응답·입력 ACK·`LCL?` 응답만 옵트인(커서 위치는 순서 뒤섞임 방지를 위해 제외). stop 이후 재사용 fd 보호. | **3초 kill(reason 4)** 제거 |
| 2 | `CaptureSession+AdaptiveBitrate.swift` | 혼잡 투표(미확정 포함) 윈도우는 상승 금지(HOLD). 고운동 상승은 3-윈도우 사다리(`canRaise`) 안에서만, 컷 이후에는 램프가 우선. | cut→raise→collapse 톱니 제거 |
| 3 | `CaptureSession+AdaptiveBitrate.swift`, `EncoderPolicy.swift` | `udpSendFailures` 증가(전송 거부/AU deadline 초과)는 하드 증거로 즉시 확정, **−40%** 컷(`retainFraction: 0.60`, 기존 −20% 유지). 4K 48 Mbps → 3 Mbps까지 6윈도우. | 붕괴 탈출 시간 10 s+ → 수 초 |
| 4 | `CaptureSession+Setup.swift` `paceUdpDatagram` | `udpLinkDegradedUntilNs`(abort 또는 >8 ms sendto 후 10 s) 동안은 배율 1×(wire rate)로 페이싱. overflow 임계 ≈65 → ≈26 Mbps. | 평균 대역을 감당하는 링크의 불필요한 abort/IDR 제거 |

테스트: `Tests/AdaptiveCongestionCutTests.swift`에 하드 컷 값·`retainFraction` 경계·재시도 백오프 추가. 이 머신(Linux)에는 Swift 툴체인이 없어 **컴파일·실행은 미검증**이다. macOS에서:

```
tools/build-macos-capture-shim.zsh congestion-cut-test /tmp/cct && /tmp/cct
tools/build-macos-capture-shim.zsh library <출력 dylib 경로>
```

## 미포함 / 후속

- Host 워치독(reason 1, 피드백 15 s 침묵)은 뷰어→Host 업링크가 병목일 때만 걸린다. 이번 변경과 무관하며 라우터 CPU 경쟁이 실제 병목이면 별도 대응이 필요하다.
- 4K video 모드 바닥 24 Mbps는 유지했다. 하드 컷이 바닥을 통과해 생존 바닥(3 Mbps)까지 내려가므로 당장 바꿀 필요가 없다.
- 뷰어 `stream_stop` 진단 기록에 종료 reason 추가(`apps/viewer-expo/src/diagnostics-recorder.core.ts`)는 이번 범위 밖. 다음 사고 때 `adb logcat -s Leftcar`의 `locally terminating stream: requestedReason=` 만으로 경로를 가릴 수 있어 우선순위가 낮다.
- Linux/Windows Host에 같은 페이싙·ABR 구조가 있으면 동일 검토가 필요하다(이번 범위는 macOS만).

## 실기기 확인 방법

1. 4K 세션에서 Host가 유튜브 전체화면 재생. Host 세션 진단의 `udpSendFailures`, bitrate 계단, recovery keyframe 수를 본다. 수정 후에는 bitrate가 −40% 계단으로 수 초 내에 안정되고 `udpSendFailures`가 0 근처로 돌아와야 한다.
2. 뷰어 `adb logcat -s Leftcar`: 수정 전 `requestedReason=4`가 반복됐다면 수정 후에는 종료 자체가 없어야 한다.
3. A/B: 로컬 영상 파일(네트워크 없음) vs 정적 화면 + 대운로드. 전자만 죽으면 콘텐츠 요인(이번 수정의 대상), 후자만 죽으면 회선 경쟁(라우터 SQM/QoS 필요).
