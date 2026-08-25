# 리서치: 다른 원격 프로그램들은 화면을 어떻게 보여주는가

작성일: 2026-08-25
기준 커밋: `d6b01613e1404c83367136b64c374ef093abbf18` (branch `feat/rn-tauri-rebuild`)
방법: docs/ 문서 3종 직접 검토 + 코드 조사 1건 + 외부 공식 문서/오픈소스 저장소 웹 조사 3건(병렬)
근거 정책: 플랫폼/제품 사실은 공식 문서 또는 저장소 코드만 근거로 쓰고, 비공개 항목은 "비공개"로 명시한다. 지연 수치는 공식 발표분만 인용한다.

## 연구 질문

현재 다른 원격 프로그램들은 어떻게 화면을 보이도록 개발했는가 — 캡처 → 부호화 → 전송 → 복호화 → 클라이언트 렌더링 파이프라인의 실제 구현 방식.

## 요약

조사한 12개 프로그램/프로토콜은 크게 세 계열로 나뉘며, 화면을 "보여주는" 방식의 스펙트럼은 **소프트웨어 프레임버퍼 blit(VNC/RFB)에서 하드웨어 디코더 → 디스플레이 서피스 직접 출력(Leftcar/Moonlight)**까지다. 핵심 발견:

1. **모든 원격 데스크톱 계열이 커서를 비디오 프레임에서 분리해 클라이언트가 로컬 렌더링한다** (RFB Cursor Pseudo-encoding, RDP Pointer Update PDU, SPICE 별도 Cursor 채널, CRD SetMouseCursor, RustDesk 33ms 폴링+캐시). 이는 느린 링크에서 체감 성능을 크게 개선하는 표준 기법이며, 현재 Leftcar의 `showsCursor=true`(커서를 비디오에 심어 전송)와 대비된다.
2. **텍스트 가독성 = 색차 무손실(4:4:4/I444)**이 업계 표준 대응이다. RDP는 AVC444(4:2:0 두 스트림 결합), CRD와 RustDesk는 VP9/AV1 profile 1(I444) 협상, Apple 화면 공유 High Performance는 4:4:4 명시. Leftcar의 H.264 4:2:0은 현재 이 기준에 미달이며 docs/02 §11의 개선 순서와 일치한다.
3. **이중 채널 전송(신뢰성 계열 + 최신 프레임 우선 계열)**이 저지연 체계의 공통 패턴이다. RDP-UDP R/L, TeamViewer reliable/unreliable DataChannel, RustDesk TCP+KCP, Moonlight ENet 제어+RTP/UDP+FEC. Leftcar의 "포인터는 최신 좌표만, 키·버튼은 ACK/재시도" 정책과 같은 구조다.
4. **캡처→인코더 GPU 제로카피**가 저지연 계열의 기본기다 (Parsec "zero-copy GPU pipeline", Sunshine VRAM 공유 핸들, CRD H.264 GPU 인코딩). macOS에서는 ScreenCaptureKit IOSurface → VideoToolbox가 그 등가 경로다.
5. **클라이언트 표시는 세 갈래**: (a) 소프트웨어 blit — VNC 프레임버퍼, SPICE Cairo, TeamViewer 웹 canvas putImageData, RustDesk PixelbufferTexture; (b) GPU 서피스/텍스처 — AnyDesk Direct3D, RustDesk GPU 텍스처, RDP 서피스 합성; (c) **하드웨어 디코더에서 디스플레이 서피스로 직접 출력** — Moonlight Android `MediaCodec.configure(format, surface)`, Leftcar `AMediaCodec_configure(…, ANativeWindow, …)`. Leftcar는 (c) 그룹이며 이는 게임 스트리밍 계열과 같은 최신 경로다.

## 상세 분석

### 계열 1: 표준 프로토콜형 (VNC/RFB, RDP, SPICE, NoMachine NX)

#### VNC / RFB (RFC 6143)
- 모델: "주어진 x,y에 픽셀 사각형 놓기" 단일 프리미티브. 사각형 묶음이 하나의 framebuffer update다.
- 변경 감지: 완전 클라이언트 pull. 서버는 "클라이언트가 관심 영역의 프레임버퍼 복사본을 유지한다"고 가정하고 증분만 보내며(§7.5.3), "서버는 요청 없는 업데이트를 보내면 안 된다"(§3). 일시적 중간 상태를 건너뛰는 것이 핵심 기법.
- 인코딩: Raw/CopyRect/Hextile/TRLE/ZRLE — 모두 무손실. 텍스트는 원본 그대로 전달된다.
- 클라이언트: 자체 프레임버퍼 사본에 사각형 반영(소프트웨어 blit가 전통).
- 커서: Cursor Pseudo-encoding — "클라이언트가 커서를 로컬로 그릴 수 있음을 선언"하면 서버는 모양(픽셀+마스크)만 보낸다. "느린 링크에서 체감 성능을 크게 개선"(§7.8.1).
- 전송: TCP 신뢰 스트림 전제.

#### Microsoft RDP
- 진화: MS-RDPBCGR slow-path(T.128 유사) → fast-path(헤더 축소로 CPU/지연 절감) → RDP 8.0+ MS-RDPEGFX 그래픽 파이프라인 확장.
- MS-RDPEGFX 모델: 클라이언트에 **오프스크린 서피스**를 만들고(CREATE/MAP_SURFACE_TO_OUTPUT) WIRE_TO_SURFACE(전송+디코딩)/SURFACE_TO_SURFACE/CACHE_TO_SURFACE 블릿. 클라이언트는 프레임버퍼가 아니라 서피스 풀을 유지·합성한다.
- 코덱: RemoteFX, RemoteFX Progressive(25%→50%→100% 점진 패스), ClearCodec, AVC420, **AVC444** — 결정적 세부: "RFX_AVC444_BITMAP_STREAM은 두 개의 RFX_AVC420_BITMAP_STREAM을 캡슐화한다. 두 YUV420p 스트림을 결합해 YUV444 프레임을 만든다." 하드웨어 4:2:0 인코더를 재사용해 4:4:4를 얻는 설계.
- 전송: TCP 기본 + MS-RDPEUDP — RDP-UDP-R(재전송 수행)/RDP-UDP-L(재전송 없음, "시의성이 재전송 회피로 보존") + FEC.
- 흐름 제어: RDPGFX_FRAME_ACKNOWLEDGE_PDU — 클라이언트가 프레임 디코딩 완료와 `queueDepth`를 보고하면 서버가 전송률 적응.
- 커서: "클라이언트는 마우스 커서를 로컬로 렌더링할 수 있다(그래픽 업데이트에 포함되지 않은 경우). 이때 서버는 Pointer Update PDU로 현재 커서 이미지를 보낸다"(MS-RDPBCGR §1.3.6).

#### SPICE
- VM 특유 구조: 게스트 QXL 드라이버가 드로잉 명령을 변환 → spice-server가 명령 트리에서 "가려진 명령을 drop" → 프로토콜 메시지. 프레임 캡처가 아니라 **드로잉 명령 스트림** 모델. "Spice는 항상 렌더링 작업을 클라이언트에 위임하려 한다".
- 코덱: 무손실 QUIC/LZ/**GLZ**(전역 히스토리 사전) + "높은 빈도로 갱신되는 영역"을 휴리스틱으로 식별해 MJPEG 비디오 스트림으로 전송(프레임 드롭 허용).
- 클라이언트: 기본 Cairo 소프트웨어 렌더링, 실험적 OpenGL/GDI 가속.
- 커서: 별도 Cursor 채널. "커서를 디스플레이에서 분리하면 응답성을 위해 커서를 우선할 수 있다".

#### NoMachine NX
- 공식 확인분: "GPU 가속 인/디코딩 지원, 항상 하드웨어 인코딩 우선"(NVENC, v10+ VA-API). "멀티미디어 데이터는 가능하면 UDP, 데이터 종류와 네트워크 상태에 따라 동적 선택, UDP 불가 시 TCP 폴백"(v8+ UDP 4000). 과거 소프트웨어 코덱 VP8, 6.6.8+ H.264 포함.
- 캡처 방식·클라이언트 렌더링 경로·커서 정책: 공개 문서 부재로 미확인.

### 계열 2: 저지연 게임 스트리밍형 (Moonlight/Sunshine, Parsec, Steam Remote Play)

#### Moonlight + Sunshine
- 캡처(호스트): Windows DXGI Desktop Duplication(완전 지원)/WGC(부분), macOS ScreenCaptureKit, Linux KMS/DRM·wlroots·X11·portal·KWin·NvFBC. `display_vram.cpp`는 캡처 텍스처를 공유 핸들+keyed mutex로 인코더에 직접 전달하는 **VRAM 제로카피 경로**.
- 인코딩: NVENC/QSV/AMF/VAAPI/VideoToolbox/Vulkan Video/소프트웨어. H.264 High(+High 4:4:4)/HEVC(Main/Main10/RExt 4:4:4)/AV1. NVENC 저지연: 무한 GOP, 재정렬 0, 룩어헤드 0, CBR, ULTRA_LOW_LATENCY 튜닝.
- 전송: 세션 설정 RTSP(ENet 신뢰 채널) + 비디오/오디오는 UDP RTP + **Reed-Solomon FEC**(nanors) — 재전송 없이 손실 복구. v0.22+ 종단간 암호화.
- 클라이언트(Android): `MediaCodecDecoderRenderer.java`가 `videoDecoder.configure(format, renderTarget.getSurface(), …)`로 **하드웨어 디코더 출력을 SurfaceView 서피스에 직접 렌더링**(버퍼 복사 없음). Choreographer 기반 최대 1프레임 버퍼링 또는 즉시 렌더+이전 프레임 드롭 모드.
- 커서: DXGI 경로는 `DXGI_OUTDUPL_POINTER_SHAPE` 처리(별도 커서 형태 전송).

#### Parsec
- 공식: 캡처→인코더 "zero-copy GPU pipeline", 하드웨어 H.264, 자체 P2P "BUD(Better User Datagrams)" — "최저 지연의 신뢰성 있는 UDP 비디오", 동적 비트레이트, DTLS 1.2, 97% NAT 통과. 클라이언트 "가능한 모든 플랫폼에서 하드웨어 디코딩" + "low level frame timing 최적화".
- 공식 지연 수치: **"Parsec은 게임에 7ms의 지연만 추가한다"**(LAN, ping/거리 제외) — 이번 조사에서 유일한 공식 ms 발표분.

#### Steam Remote Play / Steam Link
- 공식 확인분: "커스텀 저지연 네트워크 프로토콜 위의 실시간 영상 부호화", 입력/음성 귀환 "milliseconds 이내", "게임 화면만 표시(데스크톱 아님)". 클라이언트는 Steam Link 앱(iOS/Android/TV/Raspberry Pi/Meta Quest 등).
- 캡처 API·인코더·포트·FEC: Valve 비공개.

### 계열 3: 상용/오픈소스 범용형 (TeamViewer, AnyDesk, CRD, RustDesk, macOS 화면 공유)

#### TeamViewer
- 데스크톱 코덱/캡처: 비공개. 공식 보도자료 수준 — "연결 품질을 분석해 압축을 자동 조정하는 Smart adaptive compression", 하드웨어 가속 활용, 라우팅 서버 아키텍처.
- 웹 클라이언트는 배포 번들 분석으로 확인: WebRTC **DataChannel** 중심(ordered/reliable와 `maxRetransmits:0` unreliable 병행) + STUN(router*.teamviewer.com:3478). 미디어 트랙 없이 비트스트림도 DataChannel로 수신해 **wasm 내부 코덱**으로 디코딩, **canvas 2D putImageData + requestAnimationFrame**으로 blit. (소프트웨어 디코딩+소프트웨어 blit 조합의 현역 사례)

#### AnyDesk
- 자체 **DeskRT** 코덱("경쟁 제품이 못 하는 방식으로 이미지 데이터를 압축·전송"). 공식 성능: LAN 60fps, **지연 16ms 미만**, 100kb/s에서도 동작. 서버 인프라 Erlang.
- 전송: 다이렉트 P2P(TCP 7070) ↔ 릴레이(TCP 80/443/6568). TLS 1.3 + AES-256.
- 클라이언트 렌더러: **Direct3D(권장)/DirectDraw/OpenGL(실험)** 선택제.
- 커서: 숨김/항상/이동 시 표시 + Follow remote cursor/window + "Capture mouse"(로컬 입력 무효화) — 공개 문서로 가장 정밀한 커서 정책.

#### Chrome Remote Desktop
- 아키텍처(Chromium 문서): 호스트↔클라이언트 **WebRTC**. Windows 3-프로세스(daemon/network/desktop), macOS 단일 프로세스, Linux PipeWire.
- 코덱: VP8/VP9/AV1(소프트웨어, libvpx/libaom) + H.264(하드웨어, Windows 인코딩). **텍스트 선명도: SDP 프로파일로 I444(무손실 색) 협상** — "프로파일 0은 I420만 지원하므로 SDP Profile 값으로 I444 사용 여부를 나타낸다". VP9 profile 1/AV1 profile 1이 매핑.
- 커서: capturer의 `SetMouseCursor`/`SetMouseCursorPosition` — 모양과 좌표 분리 전송(코드 확인).
- 웹 클라이언트 코드는 비공개(렌더링 요소 미확인).

#### RustDesk
- 캡처(`libs/scrap`): Windows DXGI DDI+GDI 폴백, Linux X11/Wayland-PipeWire/DRM, macOS CGDisplayStream, Android MediaCodec.
- 코덱: VP8/VP9/AV1(소프트웨어) + H264/H265(hwcodec) + VRAM(GPU 텍스처) + MediaCodec. 자동 우선순위 "h265 > h264 > av1/vp9/vp8"(≤4GB 시 VP8 강등). **VP9/AV1 i444 무손실 색 지원**. 피어 디코딩 능력 협상 후 코덱 전환.
- 전송: hbbs 랑데부부(TCP/UDP 21116) 홀펀칭 → 다이렉트(TCP 또는 KCP/UDP) 또는 hbbr 릴레이(TCP 21117). "대부분의 경우 홀펀칭이 성공해 릴레이는 쓰이지 않는다".
- 클라이언트(Flutter): 두 렌더링 경로 — 소프트웨어 RGBA를 외부 텍스처에 blit(`_PixelbufferTexture`) 또는 **GPU 텍스처 0-copy**(`_GpuTexture`).
- 커서: 33ms(약 30fps) 폴링으로 모양 서비스(핸들 변경 시만 전송, 해시 캐시)와 좌표 서비스 분리. 로컬/원격 커서 독립 표시, 레티나 보정, follow 옵션. DXGI 등 커서가 프레임에 박히는 캡처는 `capture_cursor_embedded` 분기.

#### macOS 화면 공유 (참조 기준, docs/apple-screen-sharing-baseline.md와 일치)
- Standard / High Performance(Apple silicon+macOS 14+, 가상 디스플레이 1–2, 동적 해상도, HDR, 4:4:4, 30/60fps, 75Mbps 권장). Standard 품질: Adaptive/Full Quality. Observe/Control 분리. Standard 모드의 VNC 기반 여부·코덱은 현행 공개 문서에 명시 없음.

### 대조: Leftcar 현재 구현 (코드 확인)

| 단계 | 구현 | 참조 |
| --- | --- | --- |
| 캡처 | ScreenCaptureKit, 420v, queueDepth 2–3, `showsCursor=true` | `native/macos-capture-shim/Sources/CaptureShim.swift:1152-1246` |
| 인코딩 | VideoToolbox 하드웨어 H.264, RealTime, B-frame 없음, MaxFrameDelayCount=0 | `CaptureShim.swift:1834-1904` |
| 전송 | 인증 UDP, 1,200-byte fragment, IDR 요청 | docs/EVIDENCE.md E10–E15 |
| 디코딩 | `AMediaCodec_configure(codec, format, ANativeWindow, …)` — 디코더 출력이 곧 화면 | `crates/viewer-decoder/src/lib.rs:462-560` |
| 표시 | `AMediaCodec_releaseOutputBuffer(idx, true)`로 Surface 직접 렌더 | `lib.rs:702,725` |
| Surface | Kotlin `StreamActivity`가 SurfaceView의 Surface를 JNI로 Rust에 전달(로직 없는 shim) | `apps/viewer-android/.../shim/StreamActivity.kt:55-118` |
| 커서 | 로컬 커서 렌더링 없음 — 커서를 비디오에 심어 전송 | `CaptureShim.swift:1179` |
| 스케일링 | 별도 스케일 모드 없음, Surface 기본 동작 | 코드 조사 |

Leftcar의 표시 경로(하드웨어 디코더 → Surface 직접 출력)는 Moonlight Android와 동일한 최신 그룹이며, 지연 최소화 관점에서 업계 최선 관행과 일치한다.

## 아키텍처 인사이트 (Leftcar에 주는 시사점)

1. **커서 분리는 검증된 업계 표준**이고 현재 Leftcar와의 가장 큰 구조적 차이다. VNC/RDP/SPICE/CRD/RustDesk/AnyDesk 모두 모양(캐시)+좌표 분리 → 클라이언트 로컬 렌더링. 커서가 비디오에 박히면 (a) 커서 이동이 영상 FPS에 종속되고 (b) IDR 없이는 이전 프레임 커서가 잔상으로 남는다. Leftcar는 이미 입력 포인터를 2× FPS로 보내므로 역방향 좌표 채널 구조가 이미 있고, 커서 모양 캐시(RustDesk 해시 캐시 방식)는 별도 데이터그램으로 확장 가능하다. 단 Galaxy XR/Android 마우스 오버레이와의 상호작용(시스템 커서 vs 앱 내 커서)은 별도 spike가 필요하다.
2. **텍스트 선명도 로드맵 검증**: docs/02 §11의 개선 순서(native 해상도 → bitrate/QP → profile → HEVC → 부분 타일 → 4:4:4)는 CRD/RustDesk의 I444 협상, RDP AVC444, Apple High Performance 4:4:4와 정확히 같은 방향이다. H.264 4:2:0이 먼저고 4:4:4가 마지막인 우선순위도 타 프로그램의 협상 폴백 구조(h265>h264>…)와 일치한다.
3. **FEC vs IDR**: Moonlight는 Reed-Solomon 패리티로 재전송 없이 손실 복구, RDP-UDP-L도 FEC. Leftcar는 유실 감지 시 IDR 재요청(E15)인데, LAN 단절 시 IDR 대기보다 패리티 몇 개가 더 빠른 복구를 줄 수 있다. 다만 ADR-0004(bake-off 전 전송 확정 금지)가 있으므로 QUIC/WebRTC 비교 시 FEC 옵션 포함이 자연스럽다.
4. **프레임 ACK 기반 흐름 제어**(RDP queueDepth 보고)는 Host가 Viewer 디코더 상태를 이미 1Hz로 수집하는 Leftcar와 결합 여지가 있다.
5. **코덱 능력 협상은 범용 원격 데스크톱의 기본**이지만 Leftcar는 고정 H.264이다. Galaxy XR의 HEVC/AV1 사양(docs/02 §7)과 결합하면 RustDesk식 폴백 사다리가 장기 옵션이다.

## 미해결 질문

- Leftcar의 `showsCursor=true` 대신 커서 분리 채널 전환 시 Galaxy XR 입력 경로(시스템 마우스 포인터 vs 앱 렌더 커서)의 실제 동작 — 실기기 spike 필요.
- NoMachine의 캡처/렌더링 세부, Steam 캡처/인코더 세부, TeamViewer/AnyDesk 데스크톱 코덱 — 공식 문서 부재로 미확인 상태 유지.
- Apple 화면 공유 Standard 모드의 전송 프로토콜(VNC 여부) — 현행 공개 문서에 명시 없음.
- Moonlight/Steam의 구체 지연 ms — 공식 발표분 없음(Parsec 7ms만 공식).

## 주요 출처

- VNC/RFB: RFC 6143 — https://www.rfc-editor.org/rfc/rfc6143
- RDP: MS-RDPBCGR/MS-RDPEGFX/MS-RDPEUDP — https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/ , …/ms-rdpegfx/ , …/ms-rdpeudp/
- SPICE: https://www.spice-space.org/spice-for-newbies.html , https://www.spice-space.org/spice-user-manual.html
- NoMachine: https://kb.nomachine.com/AR04Q01022 , AR10T01174, TR01Q09096
- Moonlight/Sunshine: https://github.com/LizardByte/Sunshine , https://github.com/moonlight-stream/moonlight-common-c , moonlight-android `MediaCodecDecoderRenderer.java`, https://moonlight-stream.org/
- Parsec: https://parsec.app/technology
- Steam: https://store.steampowered.com/remoteplay , https://partner.steamgames.com/doc/features/remoteplay
- TeamViewer: TeamViewer 14 보도자료, 웹 클라이언트 번들(static.web.teamviewer.com)
- AnyDesk: https://anydesk.com/en/performance , support.anydesk.com (firewall/settings/display)
- CRD: chromium/remoting/docs/architecture.md, codec/webrtc_video_encoder*.cc
- RustDesk: https://github.com/rustdesk/rustdesk (libs/scrap, src/server/input_service.rs, flutter 렌더 텍스처), doc.rustdesk.com
- Apple: https://support.apple.com/guide/mac-help/share-the-screen-of-another-mac-mh14066/mac
