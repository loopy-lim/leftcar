# Leftcar 성능 매트릭스

이 절차는 1440p 기준선과 4K 후보를 같은 방식으로 비교한다. 빌드 성공이나 Host의 제출 FPS만으로 통과 판정하지 않는다. 수집기는 macOS `LeftcarPerf` 누적 카운터와 Android `LeftcarNative` epoch 로그를 동시에 저장하고, JSON 요약으로 계산한다.

## 측정 순서

1. Mac Host에서 자동 캡처 backend를 유지한 채 기기를 연결한다. ScreenCaptureKit 선택을 강제하지 않는다.
2. 기기에서 `균형 · 1440p 60fps`를 선택하고 스트림을 시작한다.
3. 스트림이 실제로 움직이는 것을 확인한 뒤 warm-up을 끝낸다. quick은 10초, final/soak은 60초 warm-up 후에만 수집기를 시작한다.
4. 수집기를 실행한다. `--output`은 생성할 report의 접두사다.

   ```sh
   tools/perf-matrix/collect-1440-4k.sh --profile balanced --duration 30 --output /tmp/leftcar-1440-rtvc-quick
   ```

5. 정지 화면 10초, 창 이동/스크롤 또는 영상 재생 10초, 마지막 안정 구간 10초를 유지한다.
6. 수집기는 `${report}.host.ndjson`, `${report}.android.log`, `${report}.summary.json`, `${report}.md`를 만든다. Host의 hardware/preset/property 결과는 `${report}.md` 체크 항목에서 별도로 확인한다.
7. 같은 절차를 `video` 프로필로 반복한다. 이 프로필은 4K video 정책을 사용한다.

## 표준 실행

quick(30초)은 10초 warm-up 뒤 실행한다.

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 30 --output /tmp/leftcar-4k-ave-quick
```

final(180초)과 soak(600초)은 각각 60초 warm-up 뒤 실행한다.

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 180 --output /tmp/leftcar-4k-ave-final
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 600 --output /tmp/leftcar-4k-ave-soak
```

## 반드시 기록할 값

| 구분 | 필드 |
| --- | --- |
| 입력 | profile, codec, width×height, target FPS, capture backend, transport |
| Host 단계 | capture FPS, encode-submit FPS, encode-output FPS, output interval p95, encode in-flight |
| Android 단계 | rendered FPS, decoder input drops, output drops, stale input drops, frame gaps |
| 지연 | capture→encode p95, encode callback p95, send syscall p95, UDP pacing p95, wire→decoder |
| 복구 | network queue/capture drops, recovery drops, IDR count, suppressed requests, FEC recovered |
| 전송 | bitrate, AU 최대 크기/fragment/parity, UDP send failures |

## 판정 규칙

- `capture FPS`가 낮으면 캡처/backend 제한이다.
- `capture`는 목표에 가깝고 `encode-output`만 낮으면 VideoToolbox 또는 in-flight 제한이다.
- Host 출력은 정상인데 Android `rendered FPS`가 낮고 decoder/output drop이 증가하면 수신 디코더·Surface 경로다.
- UDP send failure가 0이어도 receiver loss, recovery IDR, pacing p95가 증가하면 복구/수신 큐 병목으로 분류한다.
- 1440p와 4K의 한 행이라도 30초 안정 구간에서 60fps를 증명하지 못하면 `60fps 달성`으로 표시하지 않는다.

## 기계 판정 범위

`summary.json`은 누적 counter의 `last - first`와 실제 표본 사이 경과시간으로 Host capture/encode-output 및 Android rendered FPS를 계산한다. 보고된 순간 FPS는 판정에 사용하지 않는다. 800–1,200ms 1초 window의 nearest-rank p5, 1초 이상 멈춘 counter, 1.5초 초과 표본 공백/꼬리 누락도 기록한다. 표본이 둘보다 적으면 parse error로 종료하며, 0fps 통과값을 만들지 않는다.

요약은 Host output interval p50/p95, encode-output latency p95, queue-oldest 추세와 Android capture-age p50/p95, output/decoder-input/frame-gap delta, gap↔IDR frame-ID 짝을 포함한다. 55fps 후보는 Host encode-output/Android render 평균 55fps 이상, Android age p95 50ms 이하, queue가 단조 증가하지 않음, decoder-input drop delta 0, 연속 표본을 요구한다. `final4kCriteriaMet`은 여기에 59fps 평균, 55fps p5, Host 16.7/18.5ms interval, 18.5ms output latency, queue가 16.67ms를 지속 초과하지 않음, output/drop 0, gap delta 2 이하와 2 frame 이내 IDR recovery까지 평가한다.

Swift의 현재 `LeftcarPerf` 행에는 hardware/preset/property 결과가 없으므로 그 사실은 JSON만으로 통과로 만들지 않는다. `${report}.md`의 Host 확인과 실제 장치 수집은 최종 acceptance에 여전히 필요하다. macOS unified log는 지우지 않는다. acceptance 명령은 기본으로 연결된 시험 기기의 Android logcat buffer만 Android capture 직전에 지워, 요청한 수집 구간으로 Android 증거를 제한한다. 디버깅을 위해 `--no-clear`를 쓸 수 있지만, 그 artifact는 다른 입증된 구간 경계가 없으면 acceptance를 만족할 수 없다.
