//! 단일화 이전의 두 하드코딩 표를 그대로 옮겨 와 전 코드 대역(0..=65535)에서
//! 새 표와 대조한다 — 여기서 통과하면 단일화가 매핑 하나도 바꾸지 않았다는 뜻.
//! 생성된 Swift 파일이 단일 소스와 일치하는지 golden 검사도 함께 한다.

use keymap::KeyMapping;

// ---- 옛 macOS 표(native/macos-capture-shim CaptureSession+Input.swift) ----

fn old_mac(android: u16) -> Option<u16> {
    let letters: [u16; 26] = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];
    if (29..=54).contains(&android) {
        return Some(letters[(android - 29) as usize]);
    }
    let digits: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    if (7..=16).contains(&android) {
        return Some(digits[(android - 7) as usize]);
    }
    let keypad: [u16; 10] = [82, 83, 84, 85, 86, 87, 88, 89, 91, 92];
    if (144..=153).contains(&android) {
        return Some(keypad[(android - 144) as usize]);
    }
    let function_keys: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
    if (131..=142).contains(&android) {
        return Some(function_keys[(android - 131) as usize]);
    }
    let upper_function_keys: [u16; 8] = [105, 107, 113, 106, 64, 79, 80, 90];
    if (183..=190).contains(&android) {
        return Some(upper_function_keys[(android - 183) as usize]);
    }
    match android {
        19 => Some(126),
        20 => Some(125),
        21 => Some(123),
        22 => Some(124),
        23 => Some(36),
        55 => Some(43),
        56 => Some(47),
        57 => Some(58),
        58 => Some(61),
        59 => Some(56),
        60 => Some(60),
        61 => Some(48),
        62 => Some(49),
        66 => Some(36),
        67 => Some(51),
        68 => Some(50),
        69 => Some(27),
        70 => Some(24),
        71 => Some(33),
        72 => Some(30),
        73 => Some(42),
        74 => Some(41),
        75 => Some(39),
        76 => Some(44),
        92 => Some(116),
        93 => Some(121),
        111 => Some(53),
        112 => Some(117),
        113 => Some(59),
        114 => Some(62),
        115 => Some(57),
        117 => Some(55),
        118 => Some(54),
        122 => Some(115),
        123 => Some(119),
        124 => Some(114),
        143 => Some(71),
        154 => Some(75),
        155 => Some(67),
        156 => Some(78),
        157 => Some(69),
        158 => Some(65),
        159 => Some(95),
        160 => Some(76),
        161 => Some(81),
        _ => None,
    }
}

// ---- 옛 Windows 표(apps/host-desktop windows_backend/input.rs) ----

fn old_windows(android: u16) -> Option<u16> {
    if (29..=54).contains(&android) {
        return Some(0x41 + (android - 29)); // VK_A..
    }
    if (7..=16).contains(&android) {
        return Some(0x30 + (android - 7)); // VK_0..
    }
    if (131..=142).contains(&android) {
        return Some(0x70 + (android - 131)); // VK_F1..
    }
    if (144..=153).contains(&android) {
        return Some(0x60 + (android - 144)); // VK_NUMPAD0..
    }
    if (183..=190).contains(&android) {
        return Some(0x7C + (android - 183)); // VK_F13..
    }
    match android {
        19 => Some(0x26),
        20 => Some(0x28),
        21 => Some(0x25),
        22 => Some(0x27),
        23 => Some(0x0D),
        55 => Some(0xBC),
        56 => Some(0xBE),
        57 => Some(0x12),
        58 => Some(0xA5),
        59 => Some(0xA0),
        60 => Some(0xA1),
        61 => Some(0x09),
        62 => Some(0x20),
        66 => Some(0x0D),
        67 => Some(0x08),
        68 => Some(0xC0),
        69 => Some(0xBD),
        70 => Some(0xBB),
        71 => Some(0xDB),
        72 => Some(0xDD),
        73 => Some(0xDC),
        74 => Some(0xBA),
        75 => Some(0xDE),
        76 => Some(0xBF),
        92 => Some(0x21),
        93 => Some(0x22),
        111 => Some(0x1B),
        112 => Some(0x2E),
        113 => Some(0xA2),
        114 => Some(0xA3),
        115 => Some(0x14),
        117 => Some(0x5B),
        118 => Some(0x5C),
        122 => Some(0x24),
        123 => Some(0x23),
        124 => Some(0x2D),
        143 => Some(0x90),
        154 => Some(0x6F),
        155 => Some(0x6A),
        156 => Some(0x6D),
        157 => Some(0x6B),
        158 => Some(0x6E),
        159 => Some(0xBC),
        160 => Some(0x0D),
        161 => Some(0xBB),
        _ => None,
    }
}

#[test]
fn mac_table_matches_previous_hardcoded_table() {
    for code in 0..=u16::MAX {
        assert_eq!(
            keymap::mac(code),
            old_mac(code),
            "android keycode {code} mac mapping drifted"
        );
    }
}

#[test]
fn windows_table_matches_previous_hardcoded_table() {
    for code in 0..=u16::MAX {
        assert_eq!(
            keymap::windows(code),
            old_windows(code),
            "android keycode {code} windows mapping drifted"
        );
    }
}

#[test]
fn lookup_never_returns_all_none() {
    for code in 0..=u16::MAX {
        if let Some(KeyMapping { mac, windows }) = keymap::lookup(code) {
            assert!(mac.is_some() || windows.is_some(), "code {code} maps nothing");
        }
    }
}

// ---- 생성 Swift 파일 golden 검사 ----

const COMMITTED_SWIFT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos-capture-shim/Sources/Transport/AndroidKeyMap.swift"
));

#[test]
fn generated_swift_file_matches_single_source() {
    let rendered = keymap::render_swift();
    assert_eq!(
        rendered,
        COMMITTED_SWIFT,
        "AndroidKeyMap.swift가 crates/keymap과 어긋난다 — `cargo run -p keymap --bin gen-swift`으로 재생성"
    );
}
