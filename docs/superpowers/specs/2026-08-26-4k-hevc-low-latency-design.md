# 4K HEVC 저지연 스트림 설계

## 목표

4K 프로필에서 H.264 인코더 병목으로 실제 출력이 21~24fps에 머무는 문제를 해결한다. 4K는 macOS VideoToolbox HEVC 하드웨어 인코더를 우선 사용하고, Android는 `video/hevc` 저지연 디코더를 선택한다. 1080p/1440p H.264 경로와 기존 wire 호환성은 유지한다.

## 현재 근거

- Android 실기기 로그에서 입력 스트림은 `3840x2160`, 목표 `60fps`로 확인됐다.
- 동일 테스트에서 Host H.264 처리시간은 `109~132ms`, 실제 출력은 `21~24fps`였다.
- Wi-Fi RTT는 약 `11~13ms`, Host UDP syscall 전송시간은 `1ms` 미만이었다.
- 테스트 기기의 `/vendor/etc/media_codecs.xml`에는 `c2.qti.hevc.decoder.low_latency`와 HEVC `3840x2176@60` capability가 있다.

## 설계

### 코덱 선택

`CaptureSession`에 H.264/HEVC 코덱 종류를 둔다. `video` content mode이고 출력이 3840x2160 이상이면 HEVC를 먼저 시도한다. HEVC 세션 생성·설정·prepare 중 하나라도 실패하면 같은 세션 시작에서 H.264를 재시도한다. 4K 여부와 실제 선택 코덱은 Host 로그와 CFG2 설정 패킷으로 확인한다.

### wire 호환

기존 H.264 설정 패킷 `CFG`는 그대로 둔다. 새 설정 패킷은 다음 형식의 `CF2`로 정의한다.

```text
CF2 | codec:u8 | repeated { nal_length:u32 BE | Annex-B NAL }
```

`codec=1`은 H.264, `codec=2`는 HEVC다. HEVC 설정에는 VPS(32), SPS(33), PPS(34)를 모두 포함한다. 미디어 AU의 `G` fragmentation, AU ID, FEC, IDR recovery는 코덱과 무관하게 유지한다.

### macOS 인코더

HEVC는 `kCMVideoCodecType_HEVC`, `kVTProfileLevel_HEVC_Main_AutoLevel`, 동일한 realtime/no-reorder/speed-priority/max-delay 설정을 사용한다. 출력 sample의 length-prefixed NAL을 기존 Annex-B AU packetizer가 처리할 수 있게 변환한다. 설정 추출만 H.264의 SPS/PPS API와 HEVC의 VPS/SPS/PPS API로 분기한다.

### Android 디코더

공통 `VideoCodec`와 generic decoder constructor를 추가한다. H.264는 `video/avc` + `csd-0/csd-1`, HEVC는 `video/hevc` + `csd-0/csd-1/csd-2`로 `AMediaFormat`을 구성한다. 두 경로 모두 named low-latency codec을 우선하고 vendor 이름이 실패하면 MIME 선택으로 fallback한다.

CFG2 수신 전에는 기존 CFG만 H.264로 해석한다. CFG2를 받으면 codec과 parameter sets를 모아 해당 decoder를 생성한다. decoder 생성 실패는 기존 IDR recovery/error path로 돌아가며, 세션을 무한 대기시키지 않는다.

### 성공 기준

- Host 로그가 `codec=hevc`, Android 로그가 `actualCodec=c2.qti.hevc.decoder.low_latency`를 표시한다.
- `3840x2160` source에서 30초 동안 실제 render FPS가 55~60fps 범위다.
- capture→render 지연이 지속적으로 증가하지 않고 100ms 이하로 유지된다.
- H.264 1080p 회귀 테스트와 기존 CFG 수신 테스트가 통과한다.

## 범위 밖

- WebRTC/QUIC 도입
- H.264 wire 포맷 변경
- Android Kotlin/JS 계층으로 codec 데이터를 전달하는 구조 변경
- 4K를 몰래 1440p 이하로 낮추는 fallback
