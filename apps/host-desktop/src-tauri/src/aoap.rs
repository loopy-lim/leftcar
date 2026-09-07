//! Android Open Accessory negotiation and bounded bulk-link framing.

use futures_lite::future::block_on;
use nusb::transfer::{Direction, EndpointType, RequestBuffer};
use nusb::{DeviceInfo, Interface};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};

pub const ACCESSORY_VID: u16 = 0x18D1;
pub const ACCESSORY_PID_PURE: u16 = 0x2D00;
pub const ACCESSORY_PID_ADB: u16 = 0x2D01;
pub const ACCESSORY_MANUFACTURER: &str = "Leftcar";
pub const ACCESSORY_MODEL: &str = "LeftcarHost";
pub const ACCESSORY_VERSION: &str = "1";

// Keep both directions bounded by bytes as well as frame count. With the
// 16 MiB 4K access-unit ceiling, two queued media frames use at most 32 MiB.
const AOAP_MEDIA_CHANNEL_CAPACITY: usize = 2;

const REQUEST_GET_PROTOCOL: u8 = 51;
const REQUEST_SEND_STRING: u8 = 52;
const REQUEST_START: u8 = 53;
const STRING_MANUFACTURER: u16 = 0;
const STRING_MODEL: u16 = 1;
const STRING_VERSION: u16 = 3;

/// AOAP SEND STRING (control request 52) payloads are zero-terminated UTF-8
/// (256-byte max). Android stores these verbatim as the accessory identity
/// that both `accessory_filter.xml` and `UsbAccessoryModule` match against.
pub fn encode_accessory_string(value: &str) -> Vec<u8> {
    let mut encoded = value.as_bytes().to_vec();
    encoded.push(0);
    encoded
}

pub fn is_accessory_id(vid: u16, pid: u16) -> bool {
    vid == ACCESSORY_VID && (pid == ACCESSORY_PID_PURE || pid == ACCESSORY_PID_ADB)
}

pub fn find_accessory(device: &DeviceInfo) -> bool {
    is_accessory_id(device.vendor_id(), device.product_id())
}

/// Interface classes that plausibly belong to an Android device capable of
/// AOAP: vendor-specific (ADB/accessory), MTP, or wireless/RNDIS. Claiming an
/// interface on macOS detaches the active driver — HID for keyboards and
/// mice, mass storage for bridges — and visibly power-cycles the device, so
/// the AOAP probe must never reach purely HID/hub/storage devices (a real
/// incident repeatedly disconnected a USB keyboard on every `auto`-transport
/// stream start; see docs/virtual-display-removal-validation.md).
pub fn interface_class_suggests_android(interface_class: u8) -> bool {
    matches!(interface_class, 0xFF | 0x06 | 0xE0)
}

pub fn handshake_candidate(device: &DeviceInfo) -> bool {
    if find_accessory(device) {
        return false;
    }
    let mut has_interface = false;
    let mut android_shaped = false;
    for interface in device.interfaces() {
        has_interface = true;
        if interface_class_suggests_android(interface.class()) {
            android_shaped = true;
        }
    }
    has_interface && android_shaped
}

/// Negotiate AOAP on a connected, non-accessory device. The device will
/// re-enumerate after START; callers rescan and then open the accessory link.
pub fn start_accessory(device: &DeviceInfo) -> Result<(), String> {
    let timeout = std::time::Duration::from_secs(1);
    let control_interface = device
        .interfaces()
        .next()
        .map(|interface| interface.interface_number())
        .ok_or("USB device has no control interface")?;
    let device = device
        .open()
        .map_err(|error| format!("USB open failed: {error}"))?;
    // nusb exposes the device-wide control helpers on Unix, while WinUSB
    // requires a claimed interface for the same control transfers.
    let control = device
        .claim_interface(control_interface)
        .map_err(|error| format!("claim USB control interface failed: {error}"))?;
    let mut protocol = [0u8; 2];
    let received = control
        .control_in_blocking(
            nusb::transfer::Control {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: REQUEST_GET_PROTOCOL,
                value: 0,
                index: 0,
            },
            &mut protocol,
            timeout,
        )
        .map_err(|error| format!("GET PROTOCOL failed: {error}"))?;
    if received != protocol.len() || protocol[0] == 0 {
        return Err("device does not support accessory mode".into());
    }
    for (index, value) in [
        (STRING_MANUFACTURER, ACCESSORY_MANUFACTURER),
        (STRING_MODEL, ACCESSORY_MODEL),
        (STRING_VERSION, ACCESSORY_VERSION),
    ] {
        let payload = encode_accessory_string(value);
        control
            .control_out_blocking(
                nusb::transfer::Control {
                    control_type: nusb::transfer::ControlType::Vendor,
                    recipient: nusb::transfer::Recipient::Device,
                    request: REQUEST_SEND_STRING,
                    value: 0,
                    index,
                },
                &payload,
                timeout,
            )
            .map_err(|error| format!("SEND STRING failed: {error}"))?;
    }
    control
        .control_out_blocking(
            nusb::transfer::Control {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: REQUEST_START,
                value: 0,
                index: 0,
            },
            &[],
            timeout,
        )
        .map_err(|error| format!("ACCESSORY START failed: {error}"))?;
    Ok(())
}

const USB_QUEUE_DEPTH: usize = 8;
const USB_TRANSFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BulkEndpoints {
    interface: u8,
    in_endpoint: u8,
    out_endpoint: u8,
    in_packet_size: usize,
}

fn accessory_bulk_endpoints(device: &nusb::Device) -> Result<BulkEndpoints, String> {
    let configuration = device
        .active_configuration()
        .map_err(|error| format!("active USB configuration unavailable: {error}"))?;
    for interface in configuration.interfaces() {
        for setting in interface.alt_settings() {
            let mut in_endpoint = None;
            let mut out_endpoint = None;
            let mut in_packet_size = 0;
            for endpoint in setting.endpoints() {
                if endpoint.transfer_type() != EndpointType::Bulk {
                    continue;
                }
                match endpoint.direction() {
                    Direction::In if in_endpoint.is_none() => {
                        in_endpoint = Some(endpoint.address());
                        in_packet_size = endpoint.max_packet_size().max(1);
                    }
                    Direction::Out if out_endpoint.is_none() => {
                        out_endpoint = Some(endpoint.address());
                    }
                    _ => {}
                }
            }
            if let (Some(in_endpoint), Some(out_endpoint)) = (in_endpoint, out_endpoint) {
                return Ok(BulkEndpoints {
                    interface: interface.interface_number(),
                    in_endpoint,
                    out_endpoint,
                    in_packet_size,
                });
            }
        }
    }
    Err("AOAP accessory has no bulk IN/OUT interface".into())
}

/// A live AOAP accessory link. One bulk pipe carries the two logical mux
/// channels; control and media are split before they reach the proxies.
pub struct AccessoryLink {
    tx: SyncSender<usb_mux::MuxFrame>,
    pub control_rx: Receiver<Vec<u8>>,
    pub media_rx: Receiver<Vec<u8>>,
    workers: Vec<JoinHandle<()>>,
}

struct UsbSession {
    tx: SyncSender<usb_mux::MuxFrame>,
    media_subscriber: std::sync::Arc<std::sync::Mutex<Option<SyncSender<Vec<u8>>>>>,
    _workers: Vec<JoinHandle<()>>,
}

static USB_SESSION: OnceLock<std::sync::Arc<std::sync::Mutex<Option<UsbSession>>>> =
    OnceLock::new();

fn usb_session() -> std::sync::Arc<std::sync::Mutex<Option<UsbSession>>> {
    USB_SESSION
        .get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new(None)))
        .clone()
}

/// Install one enumerated accessory link and return its control channel to the
/// control relay. The media channel remains owned by the next USB stream
/// proxy, so control and video can run independently over the same bulk pipe.
pub fn install_usb_link(link: AccessoryLink) -> Receiver<Vec<u8>> {
    let AccessoryLink {
        tx,
        control_rx,
        media_rx,
        workers,
    } = link;
    let subscriber: std::sync::Arc<std::sync::Mutex<Option<SyncSender<Vec<u8>>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let dispatch_subscriber = subscriber.clone();
    let media_dispatcher = thread::Builder::new()
        .name("leftcar-aoap-media-dispatch".into())
        .spawn(move || {
            for payload in media_rx {
                let Some(sender) = dispatch_subscriber.lock().unwrap().as_ref().cloned() else {
                    continue;
                };
                if sender.send(payload).is_err() {
                    *dispatch_subscriber.lock().unwrap() = None;
                }
            }
        })
        .expect("AOAP media dispatcher thread");
    let storage = usb_session();
    let mut session = storage.lock().unwrap();
    *session = Some(UsbSession {
        tx,
        media_subscriber: subscriber,
        _workers: workers
            .into_iter()
            .chain(std::iter::once(media_dispatcher))
            .collect(),
    });
    control_rx
}

pub fn take_usb_media_channel() -> Option<(SyncSender<usb_mux::MuxFrame>, Receiver<Vec<u8>>)> {
    let storage = usb_session();
    let mut session = storage.lock().unwrap();
    let session = session.as_mut()?;
    if session.media_subscriber.lock().unwrap().is_some() {
        return None;
    }
    let (sender, receiver) = mpsc::sync_channel(256);
    *session.media_subscriber.lock().unwrap() = Some(sender);
    Some((session.tx.clone(), receiver))
}

pub fn release_usb_media_channel() {
    let storage = usb_session();
    if let Some(session) = storage.lock().unwrap().as_mut() {
        *session.media_subscriber.lock().unwrap() = None;
    };
}

pub fn usb_control_sender() -> Option<SyncSender<usb_mux::MuxFrame>> {
    let storage = usb_session();
    let session = storage.lock().unwrap();
    Some(session.as_ref()?.tx.clone())
}

pub fn has_usb_link() -> bool {
    let storage = usb_session();
    let present = storage.lock().unwrap().is_some();
    present
}

pub fn clear_usb_link() {
    let storage = usb_session();
    *storage.lock().unwrap() = None;
}

impl AccessoryLink {
    /// Open an already-enumerated AOAP accessory and start bounded bulk pumps.
    pub fn open(device: &DeviceInfo) -> Result<Self, String> {
        if !find_accessory(device) {
            return Err("device is not an Android accessory".into());
        }
        let device = device
            .open()
            .map_err(|error| format!("USB accessory open failed: {error}"))?;
        let endpoints = accessory_bulk_endpoints(&device)?;
        let interface = device
            .claim_interface(endpoints.interface)
            .map_err(|error| format!("AOAP interface claim failed: {error}"))?;
        let transfer_bytes = USB_TRANSFER_BYTES
            .div_ceil(endpoints.in_packet_size)
            .saturating_mul(endpoints.in_packet_size);

        let (tx, writer_rx) = mpsc::sync_channel::<usb_mux::MuxFrame>(AOAP_MEDIA_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::sync_channel(64);
        let (media_tx, media_rx) = mpsc::sync_channel(AOAP_MEDIA_CHANNEL_CAPACITY);
        let reader_interface = interface.clone();
        let writer_interface = interface;
        let reader = thread::Builder::new()
            .name("leftcar-aoap-reader".into())
            .spawn(move || {
                read_bulk_loop(
                    reader_interface,
                    endpoints.in_endpoint,
                    transfer_bytes,
                    control_tx,
                    media_tx,
                )
            })
            .map_err(|error| format!("AOAP reader thread failed: {error}"))?;
        let writer = thread::Builder::new()
            .name("leftcar-aoap-writer".into())
            .spawn(move || write_bulk_loop(writer_interface, endpoints.out_endpoint, writer_rx))
            .map_err(|error| format!("AOAP writer thread failed: {error}"))?;

        Ok(Self {
            tx,
            control_rx,
            media_rx,
            workers: vec![reader, writer],
        })
    }

    pub fn sender(&self) -> SyncSender<usb_mux::MuxFrame> {
        self.tx.clone()
    }
}

fn read_bulk_loop(
    interface: Interface,
    endpoint: u8,
    transfer_bytes: usize,
    control_tx: SyncSender<Vec<u8>>,
    media_tx: SyncSender<Vec<u8>>,
) {
    let mut queue = interface.bulk_in_queue(endpoint);
    for _ in 0..USB_QUEUE_DEPTH {
        queue.submit(RequestBuffer::new(transfer_bytes));
    }
    let mut decoder = usb_mux::MuxDecoder::new();
    while queue.pending() > 0 {
        let completion = block_on(queue.next_complete());
        if completion.status.is_err() {
            break;
        }
        for frame in match decoder.feed(&completion.data) {
            Ok(frames) => frames,
            Err(error) => {
                eprintln!("AOAP mux decode failed: {error}");
                return;
            }
        } {
            let channel = match frame.channel {
                usb_mux::CHANNEL_CONTROL => &control_tx,
                usb_mux::CHANNEL_MEDIA => &media_tx,
                _ => unreachable!(),
            };
            if channel.send(frame.payload).is_err() {
                return;
            }
        }
        queue.submit(RequestBuffer::reuse(completion.data, transfer_bytes));
    }
}

fn write_bulk_loop(interface: Interface, endpoint: u8, receiver: Receiver<usb_mux::MuxFrame>) {
    let mut queue = interface.bulk_out_queue(endpoint);
    while let Ok(frame) = receiver.recv() {
        let bytes = match usb_mux::encode(frame.channel, &frame.payload) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("AOAP mux encode failed: {error}");
                continue;
            }
        };
        queue.submit(bytes);
        if queue.pending() >= USB_QUEUE_DEPTH {
            let completion = block_on(queue.next_complete());
            if completion.status.is_err() {
                break;
            }
        }
    }
    while queue.pending() > 0 {
        if block_on(queue.next_complete()).status.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aoap_media_queues_are_bounded_for_4k() {
        assert_eq!(AOAP_MEDIA_CHANNEL_CAPACITY, 2);
        assert_eq!(usb_mux::MAX_FRAME_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn accessory_string_is_zero_terminated_utf8() {
        assert_eq!(encode_accessory_string("AB"), b"AB\0".to_vec());
        assert_eq!(encode_accessory_string("Leftcar"), b"Leftcar\0".to_vec());
    }

    #[test]
    fn accessory_pid_detection() {
        assert!(is_accessory_id(ACCESSORY_VID, ACCESSORY_PID_PURE));
        assert!(is_accessory_id(ACCESSORY_VID, ACCESSORY_PID_ADB));
        assert!(!is_accessory_id(ACCESSORY_VID, 0x4EE7));
        assert!(!is_accessory_id(0x05AC, ACCESSORY_PID_PURE));
    }

    #[test]
    fn aoap_probe_never_targets_hid_hub_or_storage_devices() {
        // Android-shaped interfaces: vendor-specific (ADB/accessory), MTP,
        // wireless/RNDIS.
        assert!(interface_class_suggests_android(0xFF));
        assert!(interface_class_suggests_android(0x06));
        assert!(interface_class_suggests_android(0xE0));
        // A USB keyboard/mouse (HID 0x03), hub (0x09), NVMe bridge (mass
        // storage 0x08), printer, audio, or CDC device must never receive the
        // probe — claiming their interface detaches the active driver and
        // power-cycles the device behind hubs.
        assert!(!interface_class_suggests_android(0x03));
        assert!(!interface_class_suggests_android(0x09));
        assert!(!interface_class_suggests_android(0x08));
        assert!(!interface_class_suggests_android(0x02));
        assert!(!interface_class_suggests_android(0x0A));
        assert!(!interface_class_suggests_android(0x01));
        assert!(!interface_class_suggests_android(0x07));
        assert!(!interface_class_suggests_android(0x0B));
    }

    #[test]
    fn mux_demux_preserves_interleaved_channels() {
        let mut wire = usb_mux::encode(usb_mux::CHANNEL_CONTROL, b"poll").unwrap();
        wire.extend(usb_mux::encode(usb_mux::CHANNEL_MEDIA, b"frame").unwrap());
        let mut decoder = usb_mux::MuxDecoder::new();
        let frames = decoder.feed(&wire).unwrap();
        assert_eq!(frames[0].channel, usb_mux::CHANNEL_CONTROL);
        assert_eq!(frames[1].channel, usb_mux::CHANNEL_MEDIA);
    }

    #[test]
    fn accessory_link_transfer_size_is_packet_aligned() {
        let packet = 512usize;
        let transfer = USB_TRANSFER_BYTES.div_ceil(packet) * packet;
        assert_eq!(transfer % packet, 0);
    }
}
