//! On-device decode harness (H07 device evidence).
//!
//! Usage: decode_harness <input.264>
//! Reads Annex-B H.264, decodes every AU via AMediaCodec (surfaceless),
//! prints DECODED <n> lines + final summary. Exit 0 on success.
//!
//! A tiny H.264 test pattern generator is built in: `decode_harness --gen`
//! writes a minimal SPS/PPS/IDR set that all Android AVC decoders accept.
//! Real streams come from the host encoder over adb push.

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: decode_harness <input.264> | --multi <input.264> <count>");
        std::process::exit(2);
    }
    if args[1] == "--multi" {
        let count: usize = args.get(3).and_then(|c| c.parse().ok()).unwrap_or(4);
        multi_decode(&args[2], count);
        return;
    }
    if args[1] == "--gen" {
        let mut out = std::io::stdout();
        let _ = out.write_all(minimal_stream());
        return;
    }
    let mut data = Vec::new();
    if let Err(e) = std::fs::File::open(&args[1]).and_then(|mut f| f.read_to_end(&mut data)) {
        eprintln!("read error: {e}");
        std::process::exit(2);
    }
    run(&data);
}

/// Baseline SPS/PPS + one IDR slice for 64x64. Fields are valid baseline
/// (profile_idc=66, level 10); decoders tolerate QP-only slices.
fn minimal_stream() -> &'static [u8] {
    &[
        // SPS (nal 0x67): baseline, 64x64
        0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x0a, 0xf8, 0x41, 0xa2,
        // PPS (nal 0x68)
        0x00, 0x00, 0x00, 0x01, 0x68, 0xce, 0x38, 0x80, // IDR slice (nal 0x65), minimal
        0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x0a, 0x02, 0x80, 0x10, 0x1f, 0xc6, 0x18, 0x80,
        0x00, 0x03, 0x00, 0x80, 0x00, 0x1e, 0x07, 0x81, 0x74, 0x90, 0x24, 0x08, 0x48, 0x20, 0x91,
        0x04, 0x24, 0x08, 0x82, 0x04, 0x28, 0x82, 0x00, 0x00, 0x03, 0x00, 0x08, 0x00, 0x00, 0x03,
        0x00, 0x04, 0x1f, 0xc0, 0x00, 0x14, 0x1f, 0xe0, 0x00, 0x28, 0x3f, 0xc0, 0x00, 0x50, 0x7f,
        0x80, 0x00, 0xa0, 0xff, 0x00, 0x01, 0x41, 0xf0, 0x00, 0x02, 0x83, 0xe0, 0x00, 0x05, 0x07,
        0xc0, 0x00, 0x0a, 0x0f, 0x80, 0x00, 0x14, 0x1f, 0x00,
    ]
}

fn run(data: &[u8]) {
    // Split into access units by IDR boundaries (simplified AU framing:
    // SPS+PPS+IDR = one AU; each non-IDR slice = one AU).
    let nals = viewer_decoder::split_annexb(data);
    if nals.is_empty() {
        eprintln!("NO_NALS");
        std::process::exit(1);
    }
    let mut config: (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let mut aus: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut saw_idr = false;
    for nal in &nals {
        match viewer_decoder::nal_type(nal.bytes) {
            Some(viewer_decoder::NAL_SPS) => {
                let mut v = vec![0, 0, 0, 1];
                v.extend_from_slice(nal.bytes);
                config.0 = v;
            }
            Some(viewer_decoder::NAL_PPS) => {
                let mut v = vec![0, 0, 0, 1];
                v.extend_from_slice(nal.bytes);
                config.1 = v;
            }
            Some(viewer_decoder::NAL_IDR) => {
                if saw_idr && !current.is_empty() {
                    aus.push(std::mem::take(&mut current));
                }
                current.extend_from_slice(&[0, 0, 0, 1]);
                current.extend_from_slice(nal.bytes);
                saw_idr = true;
            }
            Some(viewer_decoder::NAL_NON_IDR) => {
                current.extend_from_slice(&[0, 0, 0, 1]);
                current.extend_from_slice(nal.bytes);
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        aus.push(current);
    }
    if config.0.is_empty() || config.1.is_empty() || aus.is_empty() {
        eprintln!(
            "BAD_STREAM config_sps={} config_pps={} aus={}",
            !config.0.is_empty(),
            !config.1.is_empty(),
            aus.len()
        );
        std::process::exit(1);
    }
    println!("AUS {}", aus.len());

    unsafe {
        let mut decoder =
            match viewer_decoder::AndroidDecoder::new_h264(&config.0, &config.1, 64, 64, 0, 60) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("DECODER_CREATE_FAIL {e}");
                    std::process::exit(1);
                }
            };
        let mut rendered = 0usize;
        for (i, au) in aus.iter().enumerate() {
            let pts = i as i64 * 33_333; // ~30fps timestamps
            match decoder.feed_au(au, pts, 500_000) {
                Ok(true) => rendered += 1,
                Ok(false) => {}
                Err(e) => {
                    eprintln!("FEED_FAIL au={i} {e}");
                    std::process::exit(1);
                }
            }
            // drain a few extra outputs
            for _ in 0..4 {
                if !decoder.pump_output(50_000).unwrap_or(false) {
                    break;
                }
                rendered += 1;
            }
            if i < 3 || i % 30 == 0 {
                println!("DECODED {i}");
            }
        }
        // final drain
        for _ in 0..aus.len() * 2 {
            if !decoder.pump_output(100_000).unwrap_or(false) {
                break;
            }
            rendered += 1;
        }
        println!(
            "SUMMARY frames_rendered={} of {} aus",
            decoder.frames_rendered,
            aus.len()
        );
        let _ = rendered;
        if decoder.frames_rendered > 0 {
            println!("OK");
        } else {
            println!("NO_FRAMES_RENDERED");
            std::process::exit(1);
        }
    }
}

/// F-03 device evidence: `count` concurrent hardware decoder instances decode
/// the same golden stream simultaneously (docs/02 F-03, docs/08 H08).
fn multi_decode(path: &str, count: usize) {
    let mut data = Vec::new();
    if let Err(e) = std::fs::File::open(path).and_then(|mut f| f.read_to_end(&mut data)) {
        eprintln!("read error: {e}");
        std::process::exit(2);
    }
    let (config, aus) = match frame_stream(&data) {
        Some(v) => v,
        None => {
            eprintln!("BAD_STREAM");
            std::process::exit(1);
        }
    };
    unsafe {
        let mut decoders = Vec::new();
        for i in 0..count {
            match viewer_decoder::AndroidDecoder::new_h264(&config.0, &config.1, 320, 240, 0, 60) {
                Ok(d) => {
                    println!("INSTANCE {i} CREATED");
                    decoders.push(d);
                }
                Err(e) => {
                    println!("INSTANCE {i} FAILED {e}");
                    std::process::exit(1);
                }
            }
        }
        // round-robin feed all instances concurrently
        for (round, au) in aus.iter().enumerate() {
            for d in decoders.iter_mut() {
                let pts = round as i64 * 33_333;
                let _ = d.feed_au(au, pts, 200_000);
            }
        }
        // drain all
        for d in decoders.iter_mut() {
            for _ in 0..aus.len() * 2 {
                if !d.pump_output(100_000).unwrap_or(false) {
                    break;
                }
            }
        }
        let sum: u64 = decoders.iter().map(|d| d.frames_rendered).sum();
        println!(
            "SUMMARY instances={} frames_rendered={} of {} aus",
            count,
            sum,
            aus.len() * count
        );
        for (i, d) in decoders.iter().enumerate() {
            println!("INSTANCE {i} frames={}", d.frames_rendered);
        }
        if sum >= aus.len() as u64 * count as u64 / 2 {
            println!("OK");
        } else {
            println!("TOO_FEW_FRAMES");
            std::process::exit(1);
        }
    }
}

/// Shared AU extraction used by both run() and multi_decode().
type ConfigAndAus = ((Vec<u8>, Vec<u8>), Vec<Vec<u8>>);

fn frame_stream(data: &[u8]) -> Option<ConfigAndAus> {
    let nals = viewer_decoder::split_annexb(data);
    if nals.is_empty() {
        return None;
    }
    let mut config = (Vec::new(), Vec::new());
    let mut aus: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut saw_idr = false;
    for nal in &nals {
        match viewer_decoder::nal_type(nal.bytes) {
            Some(viewer_decoder::NAL_SPS) => {
                let mut v = vec![0, 0, 0, 1];
                v.extend_from_slice(nal.bytes);
                config.0 = v;
            }
            Some(viewer_decoder::NAL_PPS) => {
                let mut v = vec![0, 0, 0, 1];
                v.extend_from_slice(nal.bytes);
                config.1 = v;
            }
            Some(viewer_decoder::NAL_IDR) => {
                if saw_idr && !current.is_empty() {
                    aus.push(std::mem::take(&mut current));
                }
                current.extend_from_slice(&[0, 0, 0, 1]);
                current.extend_from_slice(nal.bytes);
                saw_idr = true;
            }
            Some(viewer_decoder::NAL_NON_IDR) => {
                current.extend_from_slice(&[0, 0, 0, 1]);
                current.extend_from_slice(nal.bytes);
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        aus.push(current);
    }
    if config.0.is_empty() || config.1.is_empty() || aus.is_empty() {
        return None;
    }
    Some((config, aus))
}
