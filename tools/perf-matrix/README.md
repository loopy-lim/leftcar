# Leftcar 성능 매트릭스

이 절차는 1440p H.264 기준선과 4K HEVC 후보를 같은 방식으로 비교한다.
빌드 성공이나 Host의 제출 FPS만으로 통과 판정하지 않는다. 각 행은 Host 진단 화면과 Android `LeftcarNative` 로그를 함께 기록해야 한다.

## 측정 순서

1. Mac Host에서 자동 캡처 backend를 유지한 채 기기를 연결한다. ScreenCaptureKit 선택을 강제하지 않는다.
2. 기기에서 `균형 · 1440p 60fps`를 선택하고 스트림을 시작한다.
3. 화면이 안정된 뒤 다음 명령을 실행한다.

   ```sh
   tools/perf-matrix/collect-1440-4k.sh --profile balanced --duration 30
   ```

4. 정지 화면 10초, 창 이동/스크롤 또는 영상 재생 10초, 마지막 안정 구간 10초를 유지한다.
5. Host 진단 화면의 값을 결과 Markdown에 옮긴 뒤 스트림을 정상 종료한다.
6. 같은 절차를 `video` 프로필로 반복한다. 이 프로필은 4K HEVC 정책을 사용한다.

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

수집기는 Android 로그와 장치 메타데이터만 자동으로 저장한다. Host 진단 수치는 네이티브 제어 서버의 내부 API를 임의로 재현하지 않고 사람이 결과 파일에 옮기도록 하여, 빠진 값을 성공으로 오인하지 않게 한다.
