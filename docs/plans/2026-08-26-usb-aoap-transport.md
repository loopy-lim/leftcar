# USB(AOAP) 전송 경로 구현 계획

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** adb·개발자 모드 없이 USB 케이블만으로 폰↔데스크톱을 연결하는 AOAP 액세서리 모드 전송 경로를 추가하고, `auto` 폴백을 USB 우선으로 재정의한다.

**Architecture:** AOAP bulk 파이프 위에 1바이트 채널 mux(ch0=제어, ch1=미디어)를 얹는다. 양쪽 끝에서 기존 프로토콜을 그대로 재사용한다 — Host는 loopback TCP 프록시로 CaptureShim의 기존 TCP 경로를 그대로 쓰고, Viewer는 `prepared_tcp.rs`와 동일한 브리지 구조를 fd 기반으로 재현한다. 제어 평면은 Viewer의 `127.0.0.1:7777` → ch0 → Host의 제어 디스패치로 흐른다.

**Tech Stack:** Rust(nusb 0.1, host-desktop은 별도 워크스페이스), Kotlin(UsbManager), React Native/Expo(viewer-expo), 공유 크레이트 `crates/usb-mux`.

**설계 문서:** `docs/plans/2026-08-26-usb-aoap-transport-design.md`

**검증 명령 요약:**
- 공유 크레이트/Viewer 네이티브: 저장소 루트에서 `cargo test -p usb-mux`, `cargo test -p android-viewer`
- Host: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml`
- RN/TSX 변경 후: 루트에서 `npx -y react-doctor@latest . --verbose` (100/100 필수) + viewer-expo `tsc --noEmit`
- 기기 테스트는 T4 Step 6(최초 물리 AOAP 핸드셰이크)에서 수행

**AOAP 프로토콜 상수 (구현의 사실상 유일한 외부 계약):**

| 항목 | 값 |
|---|---|
| GET PROTOCOL | control IN, request 51, value 0, index 0, 2바이트 응답(버전) |
| SEND STRING | control OUT, request 52, value 0, index = 문자열 ID(0=제조사, 1=모델, 3=버전), 페이로드 = UTF-16LE + 2바이트 NUL |
| START | control OUT, request 53, value 0, index 0, 페이로드 없음 |
| 재열거 후 | VID 0x18D1, PID 0x2D00(순수)/0x2D01(+adb), 인터페이스 0의 bulk IN/OUT |
| 식별 문자열 | Manufacturer="Leftcar", Model="LeftcarHost", Version="1" |

⚠️ nusb API는 0.1.x 기준으로 작성했다. 시그니처가 다르면 https://docs.rs/nusb 의 예제를 우선한다. UTF-16LE 문자열 인코딩은 AOAP 표준이지만 물리 폰(T4 Step 6)으로 1차 검증한다.

---

## Milestone 1: 공유 mux 크레이트

### Task 1: `crates/usb-mux` — 채널 mux 코덱

**Files:**
- Create: `crates/usb-mux/Cargo.toml`
- Create: `crates/usb-mux/src/lib.rs`
- Modify: 루트 `Cargo.toml` (workspace members에 `crates/usb-mux` 추가)

**Step 1: 크레이트 생성 + 실패 테스트 작성**

`crates/usb-mux/Cargo.toml`:

```toml
[package]
name = "usb-mux"
version = "0.1.0"
edition = "2021"
license = "MIT"

[dependencies]
```

`crates/usb-mux/src/lib.rs`:

```rust
//! Frame codec for the Leftcar USB accessory link.
//!
//! One AOAP bulk pipe carries two logical channels. Every frame is
//! `[channel:u8][length:u32 BE][payload]`. Channel 0 is the control plane
//! (newline-delimited JSON, same as the TCP control server). Channel 1 is
//! the media plane (existing length-prefixed L2 media frames, one frame
//! per mux frame — no inner length prefix).

pub const CHANNEL_CONTROL: u8 = 0;
pub const CHANNEL_MEDIA: u8 = 1;

/// Mirrors `prepared_tcp.rs` MAX_FRAME_BYTES: 2 MiB.
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxFrame {
    pub channel: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum EncodeError {
    InvalidChannel(u8),
    PayloadTooLarge(usize),
    PayloadEmpty,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::InvalidChannel(c) => write!(f, "invalid mux channel {c}"),
            EncodeError::PayloadTooLarge(n) => {
                write!(f, "mux payload {n} exceeds {MAX_FRAME_BYTES}")
            }
            EncodeError::PayloadEmpty => write!(f, "mux payload must not be empty"),
        }
    }
}

impl std::error::Error for EncodeError {}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not enough bytes yet; the caller should read more and retry.
    Incomplete,
    InvalidChannel(u8),
    LengthZero,
    LengthTooLarge(u32),
}

/// Encode one frame. Channel must be 0 or 1, payload 1..=MAX_FRAME_BYTES.
pub fn encode(channel: u8, payload: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if channel > 1 {
        return Err(EncodeError::InvalidChannel(channel));
    }
    if payload.is_empty() {
        return Err(EncodeError::PayloadEmpty);
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(EncodeError::PayloadTooLarge(payload.len()));
    }
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(channel);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Incremental decoder: feed bytes, get complete frames.
#[derive(Default)]
pub struct MuxDecoder {
    buffer: Vec<u8>,
}

impl MuxDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Append raw bytes and pop every complete frame.
    /// A protocol violation (bad channel / bad length) is fatal for the
    /// link; the caller must tear it down.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<MuxFrame>, DecodeError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            if self.buffer.len() < 5 {
                return Ok(frames);
            }
            let channel = self.buffer[0];
            if channel > 1 {
                return Err(DecodeError::InvalidChannel(channel));
            }
            let length = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap());
            if length == 0 {
                return Err(DecodeError::LengthZero);
            }
            let length = length as usize;
            if length > MAX_FRAME_BYTES {
                return Err(DecodeError::LengthTooLarge(length as u32));
            }
            if self.buffer.len() < 5 + length {
                return Ok(frames);
            }
            let payload = self.buffer[5..5 + length].to_vec();
            self.buffer.drain(..5 + length);
            frames.push(MuxFrame { channel, payload });
        }
    }

    /// Bytes buffered but not yet part of a complete frame.
    pub fn pending(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_frame() {
        let bytes = encode(CHANNEL_MEDIA, b"IDR-frames-here").unwrap();
        let mut decoder = MuxDecoder::new();
        let frames = decoder.feed(&bytes).unwrap();
        assert_eq!(frames, vec![MuxFrame { channel: CHANNEL_MEDIA, payload: b"IDR-frames-here".to_vec() }]);
    }

    #[test]
    fn roundtrip_split_across_reads() {
        let bytes = encode(CHANNEL_CONTROL, b"{\"command\":\"getStatus\"}").unwrap();
        let mut decoder = MuxDecoder::new();
        assert_eq!(decoder.feed(&bytes[..3]).unwrap(), vec![]);
        assert_eq!(decoder.pending(), 3);
        let frames = decoder.feed(&bytes[3..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].channel, CHANNEL_CONTROL);
        assert_eq!(frames[0].payload, b"{\"command\":\"getStatus\"}");
    }

    #[test]
    fn multiple_frames_in_one_feed() {
        let mut bytes = encode(CHANNEL_CONTROL, b"one").unwrap();
        bytes.extend(encode(CHANNEL_MEDIA, b"two").unwrap());
        let mut decoder = MuxDecoder::new();
        let frames = decoder.feed(&bytes).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].payload, b"one");
        assert_eq!(frames[1].payload, b"two");
    }

    #[test]
    fn rejects_invalid_channel() {
        let mut decoder = MuxDecoder::new();
        let err = decoder.feed(&[2, 0, 0, 0, 1, b'x']).unwrap_err();
        assert_eq!(err, DecodeError::InvalidChannel(2));
    }

    #[test]
    fn rejects_zero_length() {
        let mut decoder = MuxDecoder::new();
        let err = decoder.feed(&[0, 0, 0, 0, 0]).unwrap_err();
        assert_eq!(err, DecodeError::LengthZero);
    }

    #[test]
    fn rejects_oversized_length() {
        let mut decoder = MuxDecoder::new();
        let err = decoder.feed(&[1, 0xFF, 0xFF, 0xFF, 0xFF]).unwrap_err();
        assert_eq!(err, DecodeError::LengthTooLarge(0xFFFF_FFFF));
    }

    #[test]
    fn encode_validates_channel_and_size() {
        assert!(matches!(encode(7, b"x"), Err(EncodeError::InvalidChannel(7))));
        assert!(matches!(encode(0, b""), Err(EncodeError::PayloadEmpty)));
        let big = vec![0u8; MAX_FRAME_BYTES + 1];
        assert!(matches!(encode(0, &big), Err(EncodeError::PayloadTooLarge(_))));
        // Max size is accepted.
        let max = vec![1u8; MAX_FRAME_BYTES];
        assert!(encode(CHANNEL_MEDIA, &max).is_ok());
    }
}
```

루트 `Cargo.toml`의 `[workspace] members`에 `"crates/usb-mux"` 추가 (기존 패턴 확인).

**Step 2: 테스트 실패 확인**

Run: `cargo test -p usb-mux`
Expected: 크레이트가 없어 컴파일 불가 → Step 1에서 파일을 만들었으므로 바로 통과해야 정상. TDD 순서를 엄격히 지키려면 먼저 `src/lib.rs` 없이 `cargo test -p usb-mux`가 실패하는 것을 확인 후 lib.rs의 테스트만 먼저 작성한다. 여기서는 코덱이라 테스트·구현이 한 파일에 공생하므로 한 번에 작성 후:

**Step 3: 테스트 통과 확인**

Run: `cargo test -p usb-mux`
Expected: `test result: ok. 7 passed`

**Step 4: Commit**

```bash
git add crates/usb-mux Cargo.toml Cargo.lock
git commit -m "feat(usb-mux): AOAP 링크용 채널 mux 코덱"
```

---

## Milestone 2: Host — AOAP 링크 + 루프백 프록시

### Task 2: Host에 nusb 의존성 + AOAP 핸드셰이크 모듈

**Files:**
- Modify: `apps/host-desktop/src-tauri/Cargo.toml`
- Create: `apps/host-desktop/src-tauri/src/aoap.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (모듈 등록)

**Step 1: 의존성 추가**

`apps/host-desktop/src-tauri/Cargo.toml` `[dependencies]`에:

```toml
nusb = "0.1"
usb-mux = { path = "../../../../crates/usb-mux" }
```

⚠️ host-desktop은 별도 워크스페이스이므로 상대 경로가 맞는지 `control-contract = { path = "../../../crates/control-contract" }` 패턴과 대조해 확인. `../../../../crates/usb-mux`가 아닐 수 있음 — `apps/host-desktop/src-tauri`에서 `crates/usb-mux`까지는 `../../../crates/usb-mux` (src-tauri → host-desktop → apps → 루트). control-contract 경로와 동일한 깊이이므로 `../../../crates/usb-mux`를 쓴다.

수정:

```toml
nusb = "0.1"
usb-mux = { path = "../../../crates/usb-mux" }
```

**Step 2: AOAP 핸드셰이크 + 실패 테스트**

`apps/host-desktop/src-tauri/src/aoap.rs`:

```rust
//! Android Open Accessory (AOAP) link on the Host side.
//!
//! Handshake: find a USB device that answers GET PROTOCOL, send our
//! identification strings, send START, wait for re-enumeration as the
//! accessory (VID 0x18D1 / PID 0x2D00|0x2D01), claim interface 0, and
//! expose the bulk IN/OUT endpoints through a channel mux.
//!
//! Pure logic (string encoding, accessory detection) is unit-testable;
//! the device I/O path needs a physical phone (plan T4 Step 6).

use nusb::Device;
use std::time::Duration;

pub const ACCESSORY_VID: u16 = 0x18D1;
pub const ACCESSORY_PID_PURE: u16 = 0x2D00;
pub const ACCESSORY_PID_ADB: u16 = 0x2D01;

pub const ACCESSORY_MANUFACTURER: &str = "Leftcar";
pub const ACCESSORY_MODEL: &str = "LeftcarHost";
pub const ACCESSORY_VERSION: &str = "1";

/// String index IDs from the AOAP spec.
const STRING_MANUFACTURER: u16 = 0;
const STRING_MODEL: u16 = 1;
const STRING_VERSION: u16 = 3;

const REQUEST_GET_PROTOCOL: u8 = 51;
const REQUEST_SEND_STRING: u8 = 52;
const REQUEST_START: u8 = 53;

/// AOAP strings are UTF-16LE with a 2-byte NUL terminator.
pub fn encode_accessory_string(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len() * 2 + 2);
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// A device already in accessory mode (post-START re-enumeration).
pub fn find_accessory(device: &Device) -> bool {
    device.vendor_id() == ACCESSORY_VID
        && (device.product_id() == ACCESSORY_PID_PURE || device.product_id() == ACCESSORY_PID_ADB)
}

/// Whether a device is a candidate for the AOAP handshake: an interface
/// we can talk to and not already an accessory. Anything that exposes the
/// protocol counts — phone, tablet, and accessory-capable hubs.
pub fn handshake_candidate(device: &Device) -> bool {
    !find_accessory(device)
        && device
            .configurations()
            .next()
            .is_some_and(|config| !config.interfaces.is_empty())
}

/// Run the AOAP handshake on one device. Returns Ok(()) when the device
/// accepted START and will re-enumerate as an accessory (the caller then
/// re-scans for `find_accessory`).
pub async fn start_accessory(device: &Device) -> Result<(), String> {
    let timeout = Duration::from_millis(1_000);
    let interface = device
        .open()
        .map_err(|e| format!("USB open failed: {e}"))?
        .claim_interface(0)
        .map_err(|e| format!("USB claim failed: {e}"))?;

    // GET PROTOCOL: 2-byte version response. Non-accessory-aware phones
    // stall this transfer; that is a normal "not supported" answer.
    let mut protocol = [0u8; 2];
    let received = interface
        .control_in_blocking(nusb::transfer::ControlIn {
            control_type: nusb::transfer::ControlType::Vendor,
            recipient: nusb::transfer::Recipient::Device,
            request: REQUEST_GET_PROTOCOL,
            value: 0,
            index: 0,
            length: protocol.len() as u16,
        }, &mut protocol, timeout)
        .await
        .map_err(|e| format!("GET PROTOCOL failed: {e}"))?;
    if received.length != 2 || protocol[0] == 0 {
        return Err("device does not support accessory mode".into());
    }

    for (index, value) in [
        (STRING_MANUFACTURER, ACCESSORY_MANUFACTURER),
        (STRING_MODEL, ACCESSORY_MODEL),
        (STRING_VERSION, ACCESSORY_VERSION),
    ] {
        let payload = encode_accessory_string(value);
        interface
            .control_out_blocking(nusb::transfer::ControlOut {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: REQUEST_SEND_STRING,
                value: 0,
                index,
                data: &payload,
            }, timeout)
            .await
            .map_err(|e| format!("SEND STRING failed: {e}"))?;
    }

    interface
        .control_out_blocking(nusb::transfer::ControlOut {
            control_type: nusb::transfer::ControlType::Vendor,
            recipient: nusb::transfer::Recipient::Device,
            request: REQUEST_START,
            value: 0,
            index: 0,
            data: &[],
        }, timeout)
        .await
        .map_err(|e| format!("ACCESSORY START failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessory_string_is_utf16le_with_nul() {
        // "AB" -> 41 00 42 00 00 00
        assert_eq!(encode_accessory_string("AB"), vec![0x41, 0, 0x42, 0, 0, 0]);
        // Non-ASCII round-trips through UTF-16 code units.
        let encoded = encode_accessory_string("레");
        assert_eq!(encoded, vec![0x08, 0xB80 >> 8, 0, 0].iter().copied().chain([0, 0]).collect::<Vec<u8>>().drain(..).map(|_| 0).take(0).collect::<Vec<u8>>().clone());
    }
}
```

위 테스트의 두 번째 케이스는 지나치게 복잡하니 심플하게:

```rust
    #[test]
    fn accessory_string_handles_non_ascii() {
        // U+B808 "레" -> LE bytes 08 B8, then NUL NUL.
        assert_eq!(encode_accessory_string("레"), vec![0x08, 0xB8, 0x00, 0x00]);
    }

    #[test]
    fn accessory_pid_detection() {
        // Unit-test the pure classifier through fabricated ids.
        assert!(crate::aoap::is_accessory_id(0x18D1, 0x2D00));
        assert!(crate::aoap::is_accessory_id(0x18D1, 0x2D01));
        assert!(!crate::aoap::is_accessory_id(0x18D1, 0x4EE7));
        assert!(!crate::aoap::is_accessory_id(0x05AC, 0x2D00));
    }
```

`find_accessory`를 id 기반 헬퍼로 분해:

```rust
/// Pure id classifier, unit-testable without a physical device.
pub fn is_accessory_id(vid: u16, pid: u16) -> bool {
    vid == ACCESSORY_VID && (pid == ACCESSORY_PID_PURE || pid == ACCESSORY_PID_ADB)
}

pub fn find_accessory(device: &Device) -> bool {
    is_accessory_id(device.vendor_id(), device.product_id())
}
```

`lib.rs`에 `mod aoap;` 추가.

**Step 3: 테스트 실행**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml aoap`
Expected: `accessory_string_*`, `accessory_pid_detection` 통과. nusb API 시그니처 경고/에러가 나면 docs.rs/nusb 최신 예제로 `control_in_blocking`/`control_out_blocking` 호출부만 수정한다 (논리는 동일).

**Step 4: Commit**

```bash
git add apps/host-desktop/src-tauri/Cargo.toml apps/host-desktop/src-tauri/Cargo.lock apps/host-desktop/src-tauri/src/aoap.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(aoap): Host AOAP 핸드셰이크 모듈"
```

### Task 3: AOAP 링크 스트림 + 채널 디먹스 (Host)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/aoap.rs`

**Step 1: 링크 스트림 실패 테스트**

`aoap.rs` 테스트 모듈에 추가:

```rust
    #[test]
    fn demux_splits_channels_into_queues() {
        use usb_mux::{encode, MuxDecoder, CHANNEL_CONTROL, CHANNEL_MEDIA};
        use std::sync::mpsc;
        let (frames_tx, frames_rx) = mpsc::channel();
        let mut control = Vec::new();
        let mut media = Vec::new();
        // Simulate interleaved link bytes.
        let mut wire = encode(CHANNEL_CONTROL, b"poll").unwrap();
        wire.extend(encode(CHANNEL_MEDIA, b"frame1").unwrap());
        wire.extend(encode(CHANNEL_MEDIA, b"frame2").unwrap());
        let mut decoder = MuxDecoder::new();
        for frame in decoder.feed(&wire).unwrap() {
            frames_tx.send(frame).unwrap();
        }
        while let Ok(frame) = frames_rx.try_recv() {
            match frame.channel {
                CHANNEL_CONTROL => control.push(frame.payload),
                CHANNEL_MEDIA => media.push(frame.payload),
                _ => unreachable!(),
            }
        }
        assert_eq!(control, vec![b"poll".to_vec()]);
        assert_eq!(media, vec![b"frame1".to_vec(), b"frame2".to_vec()]);
    }
```

**Step 2: 링크 구현**

`aoap.rs`에 추가 (nusb 스트리밍 API 사용):

```rust
use nusb::Interface;
use tokio::sync::mpsc as async_mpsc;

/// A live accessory link: bulk pipe + channel demux.
pub struct AccessoryLink {
    /// Outgoing mux frames to write on the bulk OUT endpoint.
    tx: async_mpsc::Sender<usb_mux::MuxFrame>,
    /// Incoming control-channel payloads (channel 0).
    pub control_rx: async_mpsc::Receiver<Vec<u8>>,
    /// Incoming media-channel payloads (channel 1).
    pub media_rx: async_mpsc::Receiver<Vec<u8>>,
}

impl AccessoryLink {
    pub fn sender(&self) -> async_mpsc::Sender<usb_mux::MuxFrame> {
        self.tx.clone()
    }

    /// Attach to an already-claimed accessory interface and start pumping.
    pub async fn spawn(
        interface: Interface,
    ) -> std::io::Result<(Self, tokio::task::JoinHandle<()>)> {
        let (tx, rx) = async_mpsc::channel::<usb_mux::MuxFrame>(256);
        let (control_tx, control_rx) = async_mpsc::channel(64);
        let (media_tx, media_rx) = async_mpsc::channel(256);
        let writer = tokio::spawn(write_loop(interface.clone(), rx));
        let reader = tokio::spawn(read_loop(interface, control_tx, media_tx));
        Ok((
            Self { tx, control_rx, media_rx },
            // join both; simplest is to return one handle for the reader
            reader,
        ))
    }
}

async fn write_loop(interface: Interface, mut rx: async_mpsc::Receiver<usb_mux::MuxFrame>) {
    use futures_lite::io::AsyncWriteExt;
    while let Some(frame) = rx.recv().await {
        let bytes = match usb_mux::encode(frame.channel, &frame.payload) {
            Ok(bytes) => bytes,
            Err(_) => continue, // encoder rejects only programmer error
        };
        if interface.bulk_out(0x02, bytes).await.is_err() {
            break;
        }
    }
}

async fn read_loop(
    interface: Interface,
    control_tx: async_mpsc::Sender<Vec<u8>>,
    media_tx: async_mpsc::Sender<Vec<u8>>,
) {
    use futures_lite::io::AsyncReadExt;
    let mut decoder = usb_mux::MuxDecoder::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match interface.bulk_in(0x81, &mut buffer).await {
            Ok(n) if n > 0 => {
                if let Ok(frames) = decoder.feed(&buffer[..n]) {
                    for frame in frames {
                        let target = if frame.channel == usb_mux::CHANNEL_CONTROL {
                            &control_tx
                        } else {
                            &media_tx
                        };
                        if target.send(frame.payload).await.is_err() {
                            return;
                        }
                    }
                } else {
                    return; // protocol violation: tear down
                }
            }
            _ => return,
        }
    }
}
```

⚠️ `futures-lite` 의존성을 Cargo.toml에 추가해야 할 수 있다 (`futures-lite = "2"`). nusb 버전에 따라 `interface.bulk_out(endpoint, data)`가 완료 대기 없이 큐에 넣는 API일 수 있다 — docs.rs/nusb "Interface" 문서의 인터페이스 예제(마지막 인수 `endpoints` 벡터로 `claim_interface(0, &[0x81, 0x02])` 스타일)를 따른다. 컴파일이 되도록 nusb 최신 API에 맞춰 조정한다. 물리 검증은 T4 Step 6.

**Step 3: 테스트 실행**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml aoap`
Expected: `demux_splits_channels_into_queues` 통과

**Step 4: Commit**

```bash
git add apps/host-desktop/src-tauri/src/aoap.rs apps/host-desktop/src-tauri/Cargo.toml apps/host-desktop/src-tauri/Cargo.lock
git commit -m "feat(aoap): 액세서리 링크 스트림과 채널 디먹스"
```

### Task 4: 제어/미디어 loopback 프록시 (Host)

**개념**: CaptureShim은 loopback TCP로 미디어를 보내도록 그대로 두고(기존 TCP 경로 재사용), Host가 그 연결을 ch1로 릴레이한다. 제어 평면은 Viewer의 `127.0.0.1:7777` → ch0 → Host의 `handle_conn` 재사용.

**Files:**
- Create: `apps/host-desktop/src-tauri/src/aoap_proxy.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (`mod aoap_proxy;`)

`apps/host-desktop/src-tauri/src/aoap_proxy.rs`:

```rust
//! Loopback proxies bridging the AOAP mux channels onto existing code.
//!
//! Media: the capture shim keeps connecting to `127.0.0.1:<media_port>`
//! with its existing TCP media path. A local TCP listener accepts that
//! connection and relays frames (minus the TCP length prefix, since each
//! mux frame already carries one media frame) onto channel 1.
//!
//! Control: the viewer's control client connects to `127.0.0.1:7777`
//! THROUGH the phone (via channel 0). Wait — the control plane direction
//! is reversed: the viewer is the TCP client to the Host control server.
//! Over USB the phone cannot reach the Host's LAN listener, so the Host
//! pushes a control-proxy listener into the mux: every channel-0 frame is
//! one line of the control protocol, relayed to `ControlServer::dispatch`.

use crate::aoap::AccessoryLink;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use usb_mux::{MuxFrame, CHANNEL_CONTROL, CHANNEL_MEDIA};

/// Bridge one accepted shim TCP connection onto mux channel 1.
pub async fn bridge_media_tcp(
    mut stream: TcpStream,
    link: AccessoryLink,
) -> std::io::Result<()> {
    let mut sender = link.sender();
    let mut media_rx = link.media_rx;
    // The shim sends the LCH1 challenge framed; we must echo it back over
    // the mux. prepared_tcp.rs echoes on the same connection, so here we
    // simply relay everything bidirectionally, frame by frame.
    let mut reader = stream.clone();
    let mut writer = stream;
    let mut read_buf = vec![0u8; 16 * 1024];

    let read_task = tokio::spawn(async move {
        let mut decoder = LengthPrefixDecoder::new();
        loop {
            let n = reader.read(&mut read_buf).await?;
            if n == 0 {
                return Ok::<(), std::io::Error>(());
            }
            for payload in decoder.feed(&read_buf[..n])? {
                sender
                    .send(MuxFrame { channel: CHANNEL_MEDIA, payload })
                    .await
                    .map_err(|_| std::io::Error::other("mux closed"))?;
            }
        }
    });
    let write_task = tokio::spawn(async move {
        while let Some(payload) = media_rx.recv().await {
            let frame = [(&(payload.len() as u32)).to_be_bytes().as_slice(), &payload].concat();
            writer.write_all(&frame).await?;
        }
        Ok::<(), std::io::Error>(())
    });
    let _ = tokio::join!(read_task, write_task);
    Ok(())
}

/// Decode the shim's length-prefixed TCP media framing.
struct LengthPrefixDecoder {
    buffer: Vec<u8>,
}

impl LengthPrefixDecoder {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn feed(&mut self, bytes: &[u8]) -> std::io::Result<Vec<Vec<u8>>> {
        self.buffer.extend_from_slice(bytes);
        let mut payloads = Vec::new();
        loop {
            if self.buffer.len() < 4 {
                return Ok(payloads);
            }
            let length = u32::from_be_bytes(self.buffer[..4].try_into().unwrap()) as usize;
            if length == 0 || length > 2 * 1024 * 1024 {
                return Err(std::io::Error::other("bad media frame length"));
            }
            if self.buffer.len() < 4 + length {
                return Ok(payloads);
            }
            payloads.push(self.buffer[4..4 + length].to_vec());
            self.buffer.drain(..4 + length);
        }
    }
}
```

⚠️ 이 Task 4의 `bridge_media_tcp` 초안은 소유권 문제(하나의 `TcpStream`을 두 태스크가 나눠 쓰는 구조)와 LCH1 에코 경로(미디어 챌린지가 mux를 왕복해야 함)가 설계상 명확히 잡혀 있지 않다. 구현 시 `tokio::io::split`을 쓰고, LCH1 에코는 Viewer의 `usb_bridge`가 하도록 둔다(기존 `prepared_tcp.rs:213-235` 패턴 동일). 이 프록시는 순수 릴레이가 된다.

**Step 1: LengthPrefixDecoder 실패 테스트**

테스트 모듈:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefix_decoder_roundtrip() {
        let mut decoder = LengthPrefixDecoder::new();
        let mut wire = Vec::new();
        wire.extend(4u32.to_be_bytes());
        wire.extend(b"LCH1");
        wire.extend(3u32.to_be_bytes());
        wire.extend(b"abc");
        let payloads = decoder.feed(&wire).unwrap();
        assert_eq!(payloads, vec![b"LCH1".to_vec(), b"abc".to_vec()]);
    }

    #[test]
    fn length_prefix_decoder_rejects_zero() {
        let mut decoder = LengthPrefixDecoder::new();
        assert!(decoder.feed(&[0, 0, 0, 0]).is_err());
    }

    #[test]
    fn length_prefix_decoder_rejects_oversize() {
        let mut decoder = LengthPrefixDecoder::new();
        assert!(decoder.feed(&[0xFF, 0, 0, 1]).is_err());
    }
}
```

**Step 2: Run**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml aoap_proxy`
Expected: 3 passed

**Step 3: Commit**

```bash
git add apps/host-desktop/src-tauri/src/aoap_proxy.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(aoap): 미디어 loopback 프록시와 길이 프리픽스 디코더"
```

### Task 5: 제어 평면 프록시 — `ControlServer::dispatch` 직접 재사용

**개념**: 채널 0 프레임 = 제어 프로토콜 한 줄(JSON). Host는 AccessoryLink의 control_rx를 받아 기존 `handle_conn` 로직(인증 게이트 포함)을 재사용한다. `handle_conn`은 `TcpStream` 기반이라 `ControlServer::dispatch(&cmd, args, peer)`를 직접 호출하는 래퍼를 만든다. peer는 `"usb"`로 표시한다.

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/control.rs` — `dispatch`가 private이라면 pub(crate)로
- Create: `apps/host-desktop/src-tauri/src/aoap_control.rs` (또는 aoap_proxy.rs에 추가)

**Step 1: 실패 테스트** — 채널 0 프레임이 dispatch 래퍼로 들어가 JSON 응답이 나오는지. `beginPairing`은 loopback 전용인데 peer="usb"면 거부된다. 페어링은 USB 경로에서 어떻게 하나? **페어링은 Wi-Fi로 이미 완료된 상태를 가정한다** (USB는 페어링된 호스트와의 물리 경로 전환용). 토큰은 메모리에 있으므로 `authorize`가 통과한다. 이 시나리오를 테스트로 명시:

```rust
#[cfg(test)]
mod tests {
    use crate::aoap_control::dispatch_control_line;
    use serde_json::json;

    #[tokio::test]
    async fn usb_control_line_dispatches_authorized_commands() {
        let server = crate::control::tests::test_server(); // 기존 테스트 헬퍼 재사용 (없으면 PairingServer+SharedBackend로 구성)
        let token = server.pairing_for_test().unwrap(); // 테스트용 토큰 발급 헬퍼
        let line = json!({"command": "getStatus", "args": {}, "token": token}).to_string();
        let response = dispatch_control_line(&server, line, "usb").await;
        assert!(response.contains("\"ok\""));
    }

    #[tokio::test]
    async fn usb_control_line_rejects_unauthorized() {
        let server = crate::control::tests::test_server();
        let line = json!({"command": "getStatus", "args": {}}).to_string();
        let response = dispatch_control_line(&server, line, "usb").await;
        assert!(response.contains("unauthorized"));
    }
}
```

기존 `control.rs`의 테스트 헬퍼(`test_server` 등)가 무엇인지 먼저 grep으로 확인하고(`grep -n "mod tests" apps/host-desktop/src-tauri/src/control.rs`), 있으면 재사용, 없으면 최소 구성을 만든다.

**Step 2: 구현**

```rust
//! Channel-0 control relay: one mux frame = one control protocol line.

use crate::control::ControlServer;

pub async fn dispatch_control_line(
    server: &std::sync::Arc<ControlServer>,
    line: String,
    peer: &str,
) -> String {
    #[derive(serde::Deserialize)]
    struct Envelope {
        command: String,
        args: serde_json::Value,
        token: Option<String>,
    }
    let parsed: Result<Envelope, _> = serde_json::from_str(&line);
    let Some(envelope) = parsed.ok() else {
        return "{\"ok\":false,\"error\":\"bad request\"}".into();
    };
    if envelope.command != "pair"
        && !server.pairing.authorize(envelope.token.as_deref().unwrap_or(""))
    {
        return "{\"ok\":false,\"error\":\"unauthorized\"}".into();
    }
    let out = server.dispatch(&envelope.command, envelope.args, peer).await;
    serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
}
```

`control.rs`의 `Envelope`와 동일한 구조를 로컬로 재정의했다(기존 것이 private). `dispatch` 시그니처가 `pub`인지 확인하고 아니면 `pub(crate)`로 승격.

**Step 3: Run**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml aoap_control`
Expected: 2 passed

**Step 4: Commit**

```bash
git add apps/host-desktop/src-tauri/src/aoap_control.rs apps/host-desktop/src-tauri/src/control.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(aoap): 채널 0 제어 평면 릴레이 — 기존 dispatch/인증 재사용"
```

### Task 6: `MediaTransport`에 `Usb` 추가 + `auto` 순서 재정의 (Host)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/control.rs:42-50` (normalize), `:517-553` (attempts)
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift:42-56` (MediaTransportKind.parse)

**Step 1: 실패 테스트** — control.rs 테스트 모듈에:

```rust
#[test]
fn normalize_media_transport_accepts_usb() {
    assert_eq!(normalize_media_transport("usb"), Some("usb"));
    assert_eq!(normalize_media_transport("USB"), Some("usb"));
    assert_eq!(normalize_media_transport("auto"), Some("auto"));
    // Legacy adbTcp keeps working.
    assert_eq!(normalize_media_transport("adbTcp"), Some("adbTcp"));
}

#[test]
fn auto_attempts_usb_first() {
    // Extract the attempts-order logic into a testable pure function
    // `build_attempts(requested, wifi_candidates) -> Vec<(String, &str)>`
    // and assert on it.
    let attempts = build_attempts("auto", &["192.168.1.50".to_string()]);
    assert_eq!(attempts[0], ("127.0.0.1".to_string(), "usb"));
    assert!(attempts.iter().any(|(_, t)| *t == "tcp"));
    assert!(attempts.iter().any(|(_, t)| *t == "udp"));
    // usb must come before any wifi candidate.
    let usb_index = attempts.iter().position(|(_, t)| *t == "usb").unwrap();
    let first_tcp = attempts.iter().position(|(_, t)| *t == "tcp").unwrap();
    assert!(usb_index < first_tcp);
}
```

**Step 2: 구현** — `normalize_media_transport`에 `"usb" | "aoap" => Some("usb")` 추가 (기존 `"adbtcp" | "adb-tcp" | "usb" => Some("adbTcp")`에서 `"usb"`를 제거하고 신규 매핑). startStream attempts에서 `"usb"` 케이스: loopback 프록시 리스너 시작(미디어 포트는 Host가 임의 할당) 후 `("127.0.0.1", "usb")` 시도. `build_attempts` 순수 함수로 추출해 테스트. CaptureShim `MediaTransportKind.parse`에 `case "usb"` 추가 — `usesTCP`가 true가 되도록 하여 기존 TCP 미디어 경로(`connectTCPSocket`)를 태운다.

**Step 3: Run**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml control`
Expected: 신규 2개 포함 전체 통과

**Step 4: Commit**

```bash
git add apps/host-desktop/src-tauri/src/control.rs native/macos-capture-shim/Sources/CaptureShim.swift
git commit -m "feat(transport): USB(AOAP) 전송 옵션과 USB 우선 auto 폴백"
```

---

## Milestone 3: Viewer — AOAP 브리지

### Task 7: `usb_bridge.rs` — fd 기반 브리지 (Viewer 네이티브)

**Files:**
- Create: `native/android-viewer/src/usb_bridge.rs`
- Modify: `native/android-viewer/src/lib.rs` (모듈 등록)
- Modify: `native/android-viewer/src/jni.rs:1593` — transport 허용 목록에 `"usb"` 추가

**개념**: `prepared_tcp.rs`의 구조를 그대로 fd에 이식. 차이점: (1) 소켓이 아니라 `UsbAccessory` FileDescriptor, (2) mux 프레임을 벗겨야 함, (3) 렌더러로 가는 UDP 사이드채널은 그대로 유지(렌더러는 UDP 인터페이스를 기대).

`native/android-viewer/src/usb_bridge.rs` — `PreparedTcpBridge`를 복사해 수정하는 것보다, `prepared_tcp.rs`의 핵심 구조(스레드 2개: fd→미디어 채널, UDP→fd)를 fd 소스로 일반화한다:

```rust
//! USB accessory bridge: same shape as the TCP bridge, but the transport
//! is the UsbAccessory file descriptor and frames carry a channel mux.

use crate::prepared_tcp::MEDIA_CHANNEL_CAPACITY; // pub(crate)로 승격 필요
use std::io::{self, Read, Write};
use std::net::UdpSocket;
use std::os::fd::BorrowedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct UsbBridge {
    stop: Arc<AtomicBool>,
    control_addr: std::net::SocketAddr,
    media_rx: Receiver<Vec<u8>>,
    worker: Option<JoinHandle<()>>,
}

impl UsbBridge {
    /// `fd` is owned by UsbManager; we only borrow it.
    pub fn start(fd: i32) -> io::Result<Self> {
        let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
        //dup so the bridge outlives the Kotlin-side ParcelFileDescriptor
        let owned = fd.clone_fd()?; // libc::dup wrapper
        // ... mirror PreparedTcpBridge::bind: udp side channel,
        // media sync_channel, two threads:
        //   reader: fd -> mux decode -> ch0: nothing (viewer never
        //           receives control on this side; control client is JS)
        //           ch1: LCH1 echo back over fd, else media_tx.send
        //   writer: udp recv -> mux encode ch1 -> fd write
        todo!("Task 7 Step 3")
    }
}
```

Wait — 제어 평면 방향 재확인. Host 제어 서버는 Viewer가 클라이언트다. Viewer의 JS(`control.ts`)는 `react-native-tcp-socket`로 `127.0.0.1:7777`에 접속한다. USB 경로에서는: JS → Viewer 로컬 TCP 서버(새로 바인딩) → ch0 mux → Host dispatch. 즉 Viewer 쪽에서 제어 프록시는 **TCP 서버**를 여는 방향이다. `UsbBridge`에 로컬 제어 리스너를 추가한다:

```rust
// In UsbBridge::start, add a local control listener:
let control_listener = TcpListener::bind(("127.0.0.1", 0))?; // 포트는 JS에 보고
```

JS는 이 포트로 접속하고, 브리지는 그 연결을 ch0에 릴레이한다(개행 단위 그대로). 반대 방향(ch0 수신 → 로컬 제어 연결로 write)도 마찬가지.

**Step 1: 실패 테스트** — mux 코덱 재사용은 Task 1이 담당. 여기서는 JNI transport 목록과 브리지 생성 실패 케이스:

jni.rs 테스트 모듈(또는 신규):

```rust
#[test]
fn usb_transport_is_accepted_by_prepare() {
    // transport 문자열 검증 로직이 "usb"를 허용하는지.
    assert!(matches!("usb", "udp" | "tcp" | "adbTcp" | "usb" | "auto"));
}
```

이 테스트는 상수 비교라 자명하다. 실제 실패 테스트는 `usb_bridge::UsbBridge::start(-1)`이 에러를 반환하는 것:

`native/android-viewer/src/usb_bridge.rs` 테스트:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_rejects_invalid_fd() {
        assert!(UsbBridge::start(-1).is_err());
        // fd 0(stdin)는 valid할 수 있으므로 -1만.
    }
}
```

**Step 2: Run**

Run: `cargo test -p android-viewer usb_bridge`
Expected: `start_rejects_invalid_fd` FAIL (UsbBridge 없음)

**Step 3: 구현** — 위 구조를 완성. `prepared_tcp.rs`의 `tcp_to_media_channel`/`udp_to_tcp`를 참고하되: 리더는 `MuxDecoder`로 디먹스하고 ch1 페이로드만 미디어 채널로, LCH1은 mux 프레임에 싸서 fd로 에코. 라이터는 UDP 패킷을 `usb_mux::encode(CHANNEL_MEDIA, ...)`로 싸서 fd write. 별도 제어 스레드: 로컬 `TcpListener::bind(("127.0.0.1", 0))` 수락 → 각 연결의 라인을 ch0 mux로, ch0 수신을 연결로. JNI는 `leftcar_jni_prepare_usb(fd: i32) -> i32` 신규 심볼 + `leftcar_jni_usb_control_port() -> i32`를 노출하고, jni.rs의 기존 `prepare_udp_receiver` 호출부(`jni.rs:1593`) transport 목록에 `"usb"` 추가.

**Step 4: Run**

Run: `cargo test -p android-viewer`
Expected: 전체 통과

**Step 5: Commit**

```bash
git add native/android-viewer/src/usb_bridge.rs native/android-viewer/src/jni.rs native/android-viewer/src/lib.rs
git commit -m "feat(usb-bridge): 폰 측 AOAP fd 브리지 — mux 디코딩·LCH1 에코·제어 리스너"
```

### Task 8: Kotlin `UsbAccessoryModule` + Manifest

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/usb/UsbAccessoryModule.kt`
- Create: `apps/viewer-expo/android/app/src/main/res/xml/accessory_filter.xml`
- Modify: `apps/viewer-expo/android/app/src/main/AndroidManifest.xml`
- Modify: 뷰어 React 네이티브 패키지 등록 파일 (기존 nsd/stream 모듈 등록 패턴 확인)

`accessory_filter.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <usb-accessory manufacturer="Leftcar" model="LeftcarHost" version="1" />
</resources>
```

`AndroidManifest.xml` — MainActivity에 추가:

```xml
<intent-filter>
    <action android:name="android.hardware.usb.action.USB_ACCESSORY_ATTACHED" />
</intent-filter>
<meta-data
    android:name="android.hardware.usb.action.USB_ACCESSORY_ATTACHED"
    android:resource="@xml/accessory_filter" />
```

`UsbAccessoryModule.kt`:

```kotlin
package dev.leftcar.viewer.usb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbManager
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Owns the UsbAccessory lifecycle: resolves the accessory on ATTACHED
 * intents, opens the file descriptor, hands it to the native USB bridge,
 * and emits attach/detach events for the JS failover logic.
 */
class UsbAccessoryModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "UsbAccessory"

    private var pfd: android.os.ParcelFileDescriptor? = null

    fun onAccessoryAttached(intent: Intent) {
        val accessory = intent.getParcelableExtra<android.hardware.usb.UsbAccessory>(UsbManager.EXTRA_ACCESSORY)
            ?: return
        openAccessory(accessory)
    }

    fun openAccessory(accessory: android.hardware.usb.UsbAccessory): Boolean {
        val manager = reactApplicationContext.getSystemService(Context.USB_SERVICE) as UsbManager
        pfd = manager.openAccessory(accessory) ?: return false
        val fd = pfd?.fd ?: return false
        ViewerNative.prepareUsb(fd)
        return true
    }

    fun closeAccessory() {
        pfd?.close()
        pfd = null
    }

    override fun invalidate() {
        closeAccessory()
        super.invalidate()
    }
}
```

**Step 1: 작성 후 빌드 검증** — 물리 폰 없이는 런타임 검증 불가. `cd apps/viewer-expo && npx expo prebuild` 또는 기존 안드로이드 빌드 파이프라인(`./gradlew assembleDebug`)으로 컴파일만 확인. 빌드 시간이 길다면 이 Task는 T4(물리 검증) 마일스톤으로 묶어도 된다.

**Step 2: Commit**

```bash
git add apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/usb/ apps/viewer-expo/android/app/src/main/res/xml/accessory_filter.xml apps/viewer-expo/android/app/src/main/AndroidManifest.xml
git commit -m "feat(usb-viewer): UsbAccessoryModule와 accessory filter manifest"
```

### Task 9: RN/JS — USB 감지·제어 연결·전환 로직

**Files:**
- Modify: `apps/viewer-expo/src/control.ts` — USB 제어 포트로의 연결 분기
- Modify: `apps/viewer-expo/src/launch-stream.ts:47-81` — USB 우선 경로
- Modify: `apps/viewer-expo/app/catalog.tsx` — 전송 선택 UI (기존 `"auto"` 고정 개선)
- Create: `apps/viewer-expo/src/usb.ts` — UsbAccessory 네이티브 래퍼

핵심 흐름:
1. `usb.ts`: `UsbAccessory.getAccessoryState()` — ATTACHED면 네이티브 브리지가 열려 있는지, 제어 포트 번호 반환
2. `control.ts`: 호스트 연결 시 USB 상태 먼저 확인 → USB 있으면 `127.0.0.1:<usb_control_port>`로 접속, 아니면 기존 NSD/LAN 경로
3. `launch-stream.ts`: `prepareStream`에 전달하는 `mediaTransport`를 USB 감지 시 `"usb"`, 아니면 `"auto"` (Host의 auto는 이제 USB 우선이므로 Wi-Fi 전용으로 보내도 됨 — **결정**: Viewer는 실제 상태를 명시적으로 보낸다: USB 연결 시 `"usb"`, 아니면 `"auto"`)
4. detach 브로드캐스트 → JS 이벤트 → 진행 중 스트림 중단 후 Wi-Fi로 재연결 (v0.2 Phase C 백오프 공유). attach 브로드캐스트 → 진행 중이면 새 세션으로 마이그레이션

**Step 1: 실패 테스트** — viewer-expo의 기존 테스트 러너 확인(jest 있으면). `usb.ts`의 상태 머신(USB 연결됨 → transport 결정) 순수 함수 테스트:

```typescript
// apps/viewer-expo/src/__tests__/usb.test.ts
import { resolveTransport } from "../usb";

describe("resolveTransport", () => {
  it("prefers usb when accessory is attached", () => {
    expect(resolveTransport({ usbAttached: true })).toBe("usb");
  });
  it("falls back to auto otherwise", () => {
    expect(resolveTransport({ usbAttached: false })).toBe("auto");
  });
});
```

**Step 2: Run** — viewer-expo 테스트 명령 확인 후 실행 (jest: `npm test` / `yarn test` in apps/viewer-expo)
Expected: FAIL → 구현 후 PASS

**Step 3: 구현** — `usb.ts`, `control.ts`/`launch-stream.ts` 분기, `catalog.tsx` UI. `catalog.tsx` 변경 후 반드시:

Run: `npx -y react-doctor@latest . --verbose` (루트에서)
Expected: **100/100** — 미만이면 구현에서 해결(억제 금지)

Run: viewer-expo `tsc --noEmit`
Expected: 에러 없음

**Step 4: Commit**

```bash
git add apps/viewer-expo/src/usb.ts apps/viewer-expo/src/control.ts apps/viewer-expo/src/launch-stream.ts apps/viewer-expo/app/catalog.tsx apps/viewer-expo/src/__tests__/usb.test.ts
git commit -m "feat(usb-js): USB 우선 연결 선택과 attach/detach 전환 로직"
```

---

## Milestone 4: 전환(failover) 통합 + 물리 검증

### Task 10: USB attach/detach 핫플러그 감지 (Host)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/aoap.rs`
- Modify: `apps/host-desktop/src-tauri/src/aoap_proxy.rs`

**Step 1: 구현** — nusb hotplug 이벤트를 수동 discovery로 구독한다(지원 플랫폼: macOS/Windows/Linux). attach 시 일반 USB 장치에는 아무 AOAP 명령도 보내지 않고, 이미 액세서리로 열거된 장치만 AccessoryLink로 연다. Viewer의 실제 USB/auto 스트림 요청이 인증된 `requestUsb` 제어 명령을 보낸 경우에만 GET PROTOCOL → SEND STRING → START를 실행하고, 재열거된 액세서리를 watcher가 연다. detach → link 작업 종료 + Wi-Fi 폴백 트리거. 기존 세션 상태 머신(`Session.terminal_*`)과 통합.

물리 폰 없이 테스트 불가 — hotplug 이벤트 타입 매핑만 단위 테스트(이벤트 → 액션 순수 함수):

```rust
#[test]
fn hotplug_event_routing() {
    assert_eq!(usb_action_for(true, true), UsbAction::MigrateToUsb);
    assert_eq!(usb_action_for(true, false), UsbAction::StartUsbSession);
    assert_eq!(usb_action_for(false, _), UsbAction::FallbackToWifi);
}
```

**Step 2: Run + Commit**

```bash
git add apps/host-desktop/src-tauri/src/aoap.rs apps/host-desktop/src-tauri/src/aoap_proxy.rs
git commit -m "feat(aoap): USB 핫플러그 감지와 세션 마이그레이션 라우팅"
```

### Task 11: E2E 물리 검증 (macOS Host ↔ 실기기 폰)

**사전 준비:** macOS에서 nusb/libusb 접근 권한(첫 실행 시 시스템 설정 승인), 물리 폰 1대, USB 케이블(데이터 통신 지원).

**Step 1: AOAP 핸드셰이크 1차 검증** — Host 로그로 GET PROTOCOL → SEND STRING → START → 재열거(VID 0x18D1/PID 0x2D00|0x2D01) 확인. 실패 시 UTF-16LE 인코딩/문자열 인덱스 재확인.

**Step 2: 요청 시 실행 검증** — 폰에 뷰어 debug 빌드 설치, 케이블만 연결했을 때 ADB/AOAP가 선점되지 않는지 확인한다. Viewer에서 실제 스트림을 열 때만 `requestUsb`가 실행되고, 이후 Android 액세서리 intent/onNewIntent와 권한 흐름이 시작되는지 확인한다.

**Step 3: E2E 스트림** — USB 연결 상태에서 페어링된 호스트 선택 → 스트리밍. 프레임 흐름, 제어 응답(getStatus 2s 폴), 지연 확인.

**Step 4: 전환** — 스트리밍 중 케이블 제거 → Wi-Fi 자동 재개(목표 1–2s). 재연결 후 케이블 재연결 → USB 복귀.

**Step 5: soak** — USB 스트리밍 60분 (2026-08-26 FEC/ABR 설계의 soak 기준 공유). 메모리(RSS), 프레임 드랍 카운터, 온도 기록.

**Step 6: 수동 체크리스트** — 충전 전용 케이블(데이터 불가) 시 안내 메시지. USB 허브 경유. 폰 이미 실행 중 상태에서 연결(onNewIntent 경로).

**Commit** — 검증 결과 요약을 `docs/plans/2026-08-26-usb-aoap-transport.md` 검증 섹션에 추가:

```bash
git add docs/plans/2026-08-26-usb-aoap-transport.md
git commit -m "docs(usb): 물리 검증 결과 기록"
```

---

## 리스크와 대응

| 리스크 | 대응 |
|---|---|
| nusb API 시그니처 계획과 다름 | docs.rs/nusb 예제 우선, 물리 검증(T11) 전까지 컴파일 게이트 |
| macOS/libusb 권한 문제 | 첫 실행 시 시스템 설정 승인 안내, 실패 시 안내 메시지 |
| 폰이 AOAP 미지원(구형/일부 중국 폰) | GET PROTOCOL 실패 → USB 후보 실패로 처리, Wi-Fi 폴백 (설계 문서 에러 처리 표와 동일) |
| 충전 전용 케이블 | 디바이스 미검출 → 기존 "USB 없음" 상태와 동일하게 취급 |
| mux 프레임 오류 | 링크 전체 teardown + 재핸드셰이크 (설계 에러 처리 표) |
| 제어 평면 재사용 시 Envelope 이중 정의 | control.rs의 Envelope을 pub(crate)로 승격해 재사용 고려 |

## 완료 기준

- [ ] `cargo test -p usb-mux` 통과
- [ ] `cargo test -p android-viewer` 통과
- [ ] `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml` 통과
- [ ] react-doctor 100/100 (RN/TSX 변경 후)
- [ ] viewer-expo `tsc --noEmit` 통과
- [ ] T11 물리 검증 6단계 전부 통과 및 결과 기록

## 구현 상태 (2026-08-26)

- 소스 구현 및 빌드/단위 검증: 완료. `nusb 0.1.14`의 Windows control-transfer 제약에 맞춰 AOAP handshake는 claimed interface를 사용하고, hotplug stream 실패 시 enumeration polling으로 폴백한다.
- 계획의 최초 `take_usb_media_channel` 단회 소비 구조는 스트림 재시작 시 채널을 잃을 수 있어 persistent media dispatcher + session-scoped proxy release 구조로 보강했다.
- `nanors`는 계획 URL이 C 저장소라 Cargo dependency로 사용할 수 없었다. GPL 코드를 추가하지 않고 `crates/fec-core` 순수 Rust 구현으로 대체했다.
- T11 물리 6단계는 폰·데이터 케이블·USB 권한이 필요한 외부 게이트라 미실행 상태다. 따라서 완료 기준의 물리 항목은 체크하지 않는다.
