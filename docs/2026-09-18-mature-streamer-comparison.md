# 성숙 스트리머(Parsec/Moonlight/Sunshine/RDP) 구조 비교와 가설 판정

작성일: 2026-09-18. 상위 스펙("수준의 원격 스트리밍 구조 조사 및 개선")의 응답. 선행: [인터랙티브 페이싱 계획](2026-09-17-interactive-pacing-plan.md) — 그 판정("인코더가 아니라 호스트 전송 큐·pacing·복구")을 전제로 성숙 제품 구조와의 대조를 확정한다.

원칙에 따라 세 층위로만 기술한다: 공식 문서 / 오픈소스 실제 코드(moonlight-stream, LizardByte/Sunshine master, 2026-09-18 기준) / Leftcar 가설. Parsec은 공개 문서 이상 단정하지 않는다.

## 1. 가설 판정 (H1–H6)

| 가설 | 판정 | 근거 |
| --- | --- | --- |
| H1 per-frame AU burst | **메커니즘 확인, 계측 이번에 완비 — 실측 판정은 E-series** | interactive 큐 8프레임(`CaptureSession+NetworkQueue.swift:14-151`), AU deadline 초과→큐 flush+IDR(`CaptureSession+UdpPacket.swift`). 기존 계측은 last/max 스칼라뿐이었고 이번에 `auBytesP50/95/99`·`idrBytesP50/95/99`(300표본 롤링) 추가 |
| H2 recovery amplification | **코드에 루프 존재. 경화는 이미 착수(커밋됨)** — IDR 크기 검증은 신설 분포로 | overflow/deadline→IDR 경로, retransmit ring 6MiB/250ms(`MediaRetransmitRing.swift`), NACK grace clamp(8..120ms)(`media_datagram.rs:1117`). 신설 `idrBytesP*`로 "복구 IDR이 다시 혼잡을 만드는가"를 수치로 판정 |
| H3 presentation pacing | **이미 해결됨 — Moonlight와 동 계열, Leftcar가 더 일반화** | Moonlight balanced=Choreographer vsync+큐 2+drop-oldest(`MediaCodecDecoderRenderer.java:948-1077`). Leftcar: vsync 정렬 park + 수요 HWM 적응 cap 2–10(707716f, 092b53c, 92de5fa — 완벽 케이던스 99.5% 실측). freshness(최저지연)·balanced 토글 상존 |
| H4 controller oscillation | **구조적 근거 없음, 실측 대기** | ABR은 1Hz 투표+2윈도 확인+완화 램프(`CaptureSession+AdaptiveBitrate.swift`), QP·quality는 별도 게이트. 독립 진동의 관측은 없음. 브리프의 fixed-mode baseline(E1b)이 여전히 유효한 판정 실험 |
| H5 VideoToolbox config | **대부분 이미 충족 — 유일한 격차(DRL 윈도)를 이번에 스위치화** | `EnableLowLatencyRateControl`(11.3+)·RealTime·AllowFrameReordering=false·ExpectedFrameRate·PrioritizeSpeed 모두 세팅 + requested-vs-accepted 리포트(`EncoderConfigurationReport`). Apple 저지연 레시피 4요소 전부 충족. 미세 차이: DataRateLimits 윈도 1초(FFmpeg 기본과 동일; Sunshine/NVENC는 1프레임 VBV) → `LEFTCAR_DRL_WINDOW_SECONDS` 추가(§3) |
| H6 quality allocation | **부분 존재. VT 한계 명시** | adaptiveQp(BaseFrameQP) 실험 경로+품질 힌트 컨트롤러 존재. VT는 프레임 QP readback 공개 API가 없어 "QP p50/p95 관찰"은 BaseFrameQP 적용값+화질 벤치마크로 간접 판정 |

## 2. 비교표 — 핵심 선택

| 관심사 | Leftcar(현재) | Moonlight(코드) | Sunshine(코드) | RDP(문서) | Parsec(공개 문서만) |
| --- | --- | --- | --- | --- | --- |
| 인코더 rate control | LowLatencyRC + ABR + DataRateLimits 1s | (호스트 아님) | CBR, VBV=1프레임(+vbv_increase), twopass half-res | AVC/HEVC 적응 품질 | 혼잡 예측 후 인코더 bitrate 조정 주장 |
| 프레임 버스트 제한 | **DRL 1s → 이번에 0.05–1s 스위치** | — | rc_buffer=bitrate/fps | — | "비디오에 버퍼 없음" 주장 |
| 손실 복구 | FEC(RS ≤8+패리티 1–4 적응) + **NACK ring(Deadline grace 8–120ms)** + IDR | FEC만 + 프레임 폐기 + IDR(재전송 없음, `RtpVideoQueue.c`) | Moonlight와 동일(호스트측) | 재전송(TCP 계열) + 캐시 | BUD에 "TCP적 신뢰성" 주장 |
| 표시 페이싱 | freshness vs vsync park(HWM 적응 2–10) | 4모드(latency/balanced/cap-fps/smoothness), 큐 2 | (클라이언트가 Moonlight) | — | — |
| 콘텐츠 인식 | dirty-region+AU EWMA motion(interactive/video), 적응 quality | — | — | 텍스트/이미지/비디오 분류·캐싱·4:4:4 | — |
| GOP/복구 | 무한 GOP + viewer IDR + ForceKeyFrame | RFI 지원 호스트는 strict 대기 해제 | 무한 GOP + on-demand IDR(pict_type=I) | — | — |

결론적 격차는 없다 — Leftcar는 NACK(재전송)에서 Moonlight보다 진보, 표시 페이싱도 동급 일반화. 남은 실측 격차는 ①전송 큐/버스트(09-17 판정 그대로) ②RF 손실 환경에서의 복구 에피소드 크기 ③4K 화질 할당(QP/품질). LTR/RFI는 VideoToolbox에 공개 API가 없어 macOS 호스트에서는 NACK+grace+IDR이 옳은 구조(Sunshine의 RFI는 NVENC 전용).

## 3. 이번 배치 (미커밋)

1. **AU/IDR 바이트 분포** — `recordAccessUnitShape`가 전체+키프레임 롤링 300표본을 수집, `auBytesP50/P95/P99`·`idrBytesP50/P95/P99` 노출(단일+split 공용 경로). motion 정책은 델타 전용 유지(복구 IDR이 high-motion 오기록하지 않게).
2. **nacksServed/nacksMissed 스키마 복구** — shim은 발행했으나 Rust StatsInfo 경계에서 drop되던 2필드를 `control-contract`/`ffi.rs`에 추가.
3. **LEFTCAR_DRL_WINDOW_SECONDS** (0.05–1.0s clamp, 기본 1s=기존) — 6개 DataRateLimits 적용 경로(설정 2·런타임 갱신 4)가 단일 헬퍼로 일원화. `dataRateLimitWindowMs`로 실적용값 상시 노출.

게이트: shim `policy-test` 통과(신규 clamp·스케일링·퍼센타일 단언 포함), 최적화 shim 라이브러리 빌드 통과, `cargo test -p control-contract` 20 passed, `cargo test --lib`(leftcar-host-desktop) 243 passed. getStatus 신규 필드는 호스트 앱 재빌드 전까지 0/None으로 보인다(기존 패턴과 동일).

## 4. 다음 실험

E1–E8(09-17 계획)에 추가:

| # | 실험 | 판정 |
| --- | --- | --- |
| E9 | `LEFTCAR_DRL_WINDOW_SECONDS=0.25` vs 미설정(1s) — E1 baseline 위에서 | auBytesP99·idrBytesP99 감소, 복구 에피소드/gap 감소. Kbps·qualityHint 열화나 복구 IDR 품질 악화가 보이면 기각(0.05s는 IDR 캡 목적 하한) |
| E10 | E-series 계측으로 H1 판정 — auBytesP95/P99와 gap·복구 상관 | p99 버스트 프레임 주변의 큐 age·손실 상관이 H1의 최종 증거 |

4K 화질(Parec 대비) 벤치마크와 fixed-mode baseline(E1b)은 실기+사용자 일정이 필요하여 병행 대기. viewer→host 제어줄 12MiB 상한(`control.rs:348`, setClipboard 8MiB base64용) 초과 경로 존재 여부 추적은 별도 세션 과제로 유지.
