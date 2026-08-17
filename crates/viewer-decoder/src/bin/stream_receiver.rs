//! On-device UDP receive -> AMediaCodec decode -> render to an ANativeWindow
//! obtained via ANativeWindow_acquire? No — a plain binary cannot own a Java
//! Surface. Instead this receiver decodes and, when --surface <handle> is
//! passed by the app (from ANativeWindow_fromSurface), renders into it.
//!
//! Standalone mode (no surface): decodes and counts frames — proves the
//! network->decode pipeline E2E on device. The app integration passes the
//! surface handle through leftcar_jni_attach.
//!
//! Usage: stream_receiver <port> [seconds]

use std::net::UdpSocket;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: stream_receiver <port> [seconds]");
        std::process::exit(2);
    }
    let port: u16 = args[1].parse().unwrap_or(5000);
    let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);

    let sock = UdpSocket::bind(format!("0.0.0.0:{port}")).expect("bind");
    sock.set_read_timeout(Some(std::time::Duration::from_millis(500))).ok();
    println!("LISTENING udp/{port}");

    let mut sps: Vec<u8> = Vec::new();
    let mut pps: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 65536];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds + 5);

    // phase 1: wait for CONFIG
    while std::time::Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, _)) if n >= 3 && &buf[..3] == b"CFG" => {
                // parse: [C][F][G][len:u32BE][nal...]* where each nal has 00 00 00 01 prefix
                let mut off = 3usize;
                while off + 4 <= n {
                    let len = u32::from_be_bytes(
                        buf[off..off + 4].try_into().unwrap(),
                    ) as usize;
                    off += 4;
                    if off + len > n { break; }
                    let nal = &buf[off..off + len];
                    // strip the 4-byte start code for csd
                    if nal.len() > 4 {
                        let t = viewer_decoder::nal_type(&nal[4..]);
                        if t == Some(viewer_decoder::NAL_SPS) {
                            sps = nal.to_vec();
                        } else if t == Some(viewer_decoder::NAL_PPS) {
                            pps = nal.to_vec();
                        }
                    }
                    off += len;
                }
                if !sps.is_empty() && !pps.is_empty() {
                    println!("CONFIG received sps={}B pps={}B", sps.len(), pps.len());
                    break;
                }
            }
            _ => {}
        }
    }
    if sps.is_empty() || pps.is_empty() {
        println!("NO_CONFIG");
        std::process::exit(1);
    }

    unsafe {
        let mut decoder = match viewer_decoder::AndroidDecoder::new_h264(&sps, &pps, 320, 240, 0) {
            Ok(d) => d,
            Err(e) => {
                println!("DECODER_FAIL {e}");
                std::process::exit(1);
            }
        };
        println!("DECODER_READY {:?}", decoder.size());
        let start = std::time::Instant::now();
        let mut aus = 0u64;
        let mut bytes = 0u64;
        while start.elapsed().as_secs() < seconds {
            let (n, _) = match sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if n < 10 || &buf[..2] != b"AU" {
                continue;
            }
            // header: 'A''U' pts:u64 -> AU starts at 10
            let au = &buf[10..n];
            if au.is_empty() {
                continue;
            }
            aus += 1;
            bytes += au.len() as u64;
            // feed; ignore late-input for pacing
            let _ = decoder.feed_au(au, (aus * 33333) as i64, 0);
            let _ = decoder.pump_output(0);
            if aus % 30 == 0 {
                println!(
                    "RX aus={} rendered={} rate={:.1}fps",
                    aus,
                    decoder.frames_rendered,
                    aus as f64 / start.elapsed().as_secs_f64()
                );
            }
        }
        // final drain
        for _ in 0..120 {
            if !decoder.pump_output(50_000).unwrap_or(false) {
                break;
            }
        }
        println!(
            "SUMMARY received_aus={} rendered={} bytes={} elapsed={:.1}s",
            aus,
            decoder.frames_rendered,
            bytes,
            start.elapsed().as_secs_f64()
        );
        if decoder.frames_rendered > 0 {
            println!("OK");
        } else {
            println!("NO_FRAMES");
            std::process::exit(1);
        }
    }
}
