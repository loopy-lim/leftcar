//! In-memory peer for the Swift media interoperability gate. Uses only the
//! checked-in public fixture; never accepts a real session key or opens sockets.
use secure_channel::{media_keys, DatagramSealer};

fn fixture(name: &str) -> Vec<u8> {
    let value = include_str!("../tests/fixtures/media-wire-v1.txt")
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
        .expect("fixture field");
    hex::decode(value).expect("fixture hex")
}

fn main() {
    let key: [u8; 32] = fixture("media_key_hex").try_into().expect("32-byte key");
    let plaintext = fixture("plaintext_hex");
    let keys = media_keys(&key);
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode] if mode == "seal-c2s" => {
            let frame = DatagramSealer::new(keys.c2s)
                .seal(&plaintext)
                .expect("seal fixture");
            println!("{}", hex::encode(frame));
        }
        [mode, frame] if mode == "open-s2c" => {
            let frame = hex::decode(frame).expect("frame hex");
            let receiver = DatagramSealer::new(keys.s2c);
            match receiver.open(&frame) {
                Ok(opened) if opened == plaintext => {
                    assert!(receiver.open(&frame).is_err(), "reject replay");
                    assert!(DatagramSealer::new(keys.c2s).open(&frame).is_err());
                    println!("{}", hex::encode(opened));
                }
                result => {
                    eprintln!("Swift-to-Rust media frame rejected: {result:?}");
                    std::process::exit(1);
                }
            }
        }
        _ => panic!("usage: media_interop <seal-c2s|open-s2c FRAME_HEX>"),
    }
}
