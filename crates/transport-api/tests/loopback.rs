//! L5 loopback integration (docs/05 §5):
//! FakeEncoder -> packetize -> transport -> assembler -> FakeDecoder.
//!
//! Verifies handshake, 1/4 source multiplex, outage recovery, source close
//! isolation on InMemory and Simulated links.

use bytes::Bytes;
use domain::ids::{SessionId, SourceId, StreamInstanceId};
use domain::lease::LeaseTable;
use media_model::{
    packetize, AssembledOutput, CodecProfile, EncodedFrame, FragmentAssembler, FragmentHeader,
    FrameKind, StreamEpoch,
};
use std::time::Duration;
use transport_api::{simulated_pair, InMemoryTransport, LinkProfile, TransportEvent};

struct FakeEncoder {
    epoch: u32,
    next_frame_id: u64,
}

impl FakeEncoder {
    fn new() -> Self {
        Self { epoch: 1, next_frame_id: 1 }
    }

    fn encode(&mut self, source: &SourceId, session: &SessionId, kind: FrameKind, content: &[u8]) -> EncodedFrame {
        let id = self.next_frame_id;
        self.next_frame_id += 1;
        EncodedFrame {
            session_id: session.clone(),
            source_id: source.clone(),
            stream_epoch: StreamEpoch(self.epoch),
            frame_id: id,
            kind,
            codec: CodecProfile::AvcBaseline,
            capture_time_host_ns: id * 1_000_000,
            encode_done_host_ns: id * 1_000_000 + 500_000,
            width: 1920,
            height: 1080,
            payload: Bytes::copy_from_slice(content),
        }
    }
}

#[derive(Default)]
struct FakeDecoder {
    decoded: Vec<(SourceId, u64, FrameKind)>,
}

impl FakeDecoder {
    fn feed(&mut self, frame: &EncodedFrame) {
        self.decoded.push((frame.source_id.clone(), frame.frame_id, frame.kind));
    }
}

fn frag_wire(header: &FragmentHeader, payload: &Bytes) -> Bytes {
    let mut out = serde_json::to_vec(header).unwrap();
    out.push(b'|');
    out.extend_from_slice(payload);
    Bytes::from(out)
}

fn frag_unwire(bytes: &Bytes) -> (FragmentHeader, Bytes) {
    let all = bytes.as_ref();
    let sep = all.iter().position(|&b| b == b'|').expect("separator");
    let header: FragmentHeader = serde_json::from_slice(&all[..sep]).unwrap();
    (header, Bytes::copy_from_slice(&all[sep + 1..]))
}

#[test]
fn inmemory_loopback_one_source_end_to_end() {
    let session = SessionId::from_raw("sess").unwrap();
    let source = SourceId::from_raw("solo").unwrap();
    let mut transport = InMemoryTransport::new();
    let mut enc = FakeEncoder::new();
    let mut asm = FragmentAssembler::new(session.clone(), CodecProfile::AvcBaseline);
    let mut dec = FakeDecoder::default();

    // handshake via framed control envelope
    let hello = network_protocol::ControlEnvelope {
        protocol_version: 1,
        session_id: session.0.clone(),
        request_id: "hello".into(),
        monotonic_sequence: 0,
        kind: network_protocol::ControlKind::SessionPing,
        payload: vec![],
    };
    transport.send_control(Bytes::from(network_protocol::frame_control(&hello).unwrap()));
    match transport.recv().unwrap() {
        TransportEvent::Control(b) => {
            let (parsed, _) = network_protocol::parse_control(&b).unwrap();
            assert_eq!(parsed.kind, network_protocol::ControlKind::SessionPing);
        }
        other => panic!("expected control hello, got {other:?}"),
    }

    for round in 0..5u64 {
        let kind = if round == 0 { FrameKind::Key } else { FrameKind::Delta };
        let frame = enc.encode(&source, &session, kind, &[7u8; 700]);
        for frag in packetize(&frame, 512).unwrap() {
            transport.send_video(source.clone(), frag_wire(&frag.header, &frag.payload));
        }
        while let Some(event) = transport.recv() {
            if let TransportEvent::Video(_, bytes) = event {
                let (header, payload) = frag_unwire(&bytes);
                let out = asm
                    .feed(media_model::Fragment { header, payload }, Duration::from_millis(round * 10))
                    .unwrap();
                if let AssembledOutput::Frame(f) = out {
                    dec.feed(&f);
                }
            }
        }
    }
    assert_eq!(dec.decoded.len(), 5, "all 5 frames decoded");
    assert_eq!(dec.decoded[0].2, FrameKind::Key);
}

#[test]
fn inmemory_loopback_four_sources_without_cross_talk() {
    let session = SessionId::from_raw("sess").unwrap();
    let sources: Vec<SourceId> = (0..4)
        .map(|i| SourceId::from_raw(&format!("m{i}")).unwrap())
        .collect();
    let mut transport = InMemoryTransport::new();
    let mut enc = FakeEncoder::new();
    let mut asms: Vec<FragmentAssembler> = sources
        .iter()
        .map(|_| FragmentAssembler::new(session.clone(), CodecProfile::AvcBaseline))
        .collect();
    let mut decs: Vec<FakeDecoder> = (0..4).map(|_| FakeDecoder::default()).collect();

    for round in 0..3 {
        for source in &sources {
            let kind = if round == 0 { FrameKind::Key } else { FrameKind::Delta };
            let frame = enc.encode(source, &session, kind, &[0xAB; 700]);
            for frag in packetize(&frame, 512).unwrap() {
                transport.send_video(source.clone(), frag_wire(&frag.header, &frag.payload));
            }
        }
    }
    while let Some(event) = transport.recv() {
        if let TransportEvent::Video(src, bytes) = event {
            let idx = sources.iter().position(|s| *s == src).expect("known source");
            let (header, payload) = frag_unwire(&bytes);
            let out = asms[idx]
                .feed(media_model::Fragment { header, payload }, Duration::ZERO)
                .unwrap();
            if let AssembledOutput::Frame(f) = out {
                decs[idx].feed(&f);
            }
        }
    }
    for (i, dec) in decs.iter().enumerate() {
        assert_eq!(dec.decoded.len(), 3, "source {i} decoded all frames");
        assert_eq!(dec.decoded[0].2, FrameKind::Key, "source {i} starts with key");
        // no cross talk: every decoded frame belongs to this source
        assert!(dec.decoded.iter().all(|(s, _, _)| *s == sources[i]));
    }
}

#[test]
fn simulated_bad_wifi_delivers_nearly_all_frames() {
    let session = SessionId::from_raw("sess").unwrap();
    let source = SourceId::from_raw("lossy").unwrap();
    let (mut tx, mut rx) = simulated_pair(LinkProfile::bad_wifi(), 7);
    let mut enc = FakeEncoder::new();
    let mut asm = FragmentAssembler::new(session.clone(), CodecProfile::AvcBaseline);
    let mut dec = FakeDecoder::default();
    let mut idr_requests = 0;

    for round in 0..40u64 {
        let kind = if round == 0 { FrameKind::Key } else { FrameKind::Delta };
        let frame = enc.encode(&source, &session, kind, &[round as u8; 300]);
        for frag in packetize(&frame, 512).unwrap() {
            tx.send_video(source.clone(), frag_wire(&frag.header, &frag.payload));
        }
    }
    for event in rx.advance(Duration::from_millis(3_000)) {
        if let TransportEvent::Video(_, bytes) = event {
            let (header, payload) = frag_unwire(&bytes);
            match asm.feed(media_model::Fragment { header, payload }, Duration::ZERO).unwrap() {
                AssembledOutput::Frame(f) => dec.feed(&f),
                AssembledOutput::RequestIdr { .. } => idr_requests += 1,
                AssembledOutput::Dropped => {}
            }
        }
    }
    assert!(
        dec.decoded.len() >= 35,
        "3% loss should still deliver most complete frames: {}",
        dec.decoded.len()
    );
    let _ = idr_requests;
}

#[test]
fn outage_then_recovery_resumes_with_keyframe() {
    let session = SessionId::from_raw("sess").unwrap();
    let source = SourceId::from_raw("outage").unwrap();
    let (mut tx, mut rx) = simulated_pair(LinkProfile::outage(Duration::ZERO, Duration::from_secs(5)), 1);
    let mut enc = FakeEncoder::new();
    let mut asm = FragmentAssembler::new(session.clone(), CodecProfile::AvcBaseline);
    let mut dec = FakeDecoder::default();

    let during = enc.encode(&source, &session, FrameKind::Key, &[9u8; 300]);
    for frag in packetize(&during, 512).unwrap() {
        tx.send_video(source.clone(), frag_wire(&frag.header, &frag.payload));
    }
    assert!(rx.advance(Duration::from_millis(100)).is_empty(), "outage blocks all");

    // advance past the outage window, then resend a keyframe
    let _ = rx.advance(Duration::from_secs(6));
    let after = enc.encode(&source, &session, FrameKind::Key, &[8u8; 300]);
    for frag in packetize(&after, 512).unwrap() {
        tx.send_video(source.clone(), frag_wire(&frag.header, &frag.payload));
    }
    let events = rx.advance(Duration::from_secs(6));
    assert!(!events.is_empty(), "post-outage keyframe delivers");
    for event in events {
        if let TransportEvent::Video(_, bytes) = event {
            let (header, payload) = frag_unwire(&bytes);
            if let AssembledOutput::Frame(f) =
                asm.feed(media_model::Fragment { header, payload }, Duration::ZERO).unwrap()
            {
                dec.feed(&f);
            }
        }
    }
    assert_eq!(dec.decoded.len(), 1);
    assert_eq!(dec.decoded[0].2, FrameKind::Key, "recovery starts from IDR (NFR-004)");
}

#[test]
fn source_close_isolation_via_leases() {
    let mut leases = LeaseTable::new();
    let a = SourceId::from_raw("a").unwrap();
    let b = SourceId::from_raw("b").unwrap();
    let i1 = StreamInstanceId::from_raw("i1").unwrap();
    let i2 = StreamInstanceId::from_raw("i2").unwrap();
    leases.acquire(a.clone(), i1.clone());
    leases.acquire(b.clone(), i2.clone());

    let pending = leases.release(&a, &i1, Duration::ZERO, Duration::from_secs(1)).unwrap();
    assert!(pending.is_some(), "source A schedules stop");
    assert!(leases.stop_elapsed(&a, Duration::from_secs(2)).is_some(), "A stops after debounce");
    assert!(leases.stop_elapsed(&b, Duration::from_secs(2)).is_none(), "B unaffected");
    assert_eq!(leases.lease_count(&b), 1);
}
