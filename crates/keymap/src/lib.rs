//! 안드로이드 키코드 → 호스트 플랫폼 키코드 기본표의 단일 소스.
//!
//! Windows 호스트(apps/host-desktop `windows_backend`)는 이 크레이트를 직접
//! 쓰고, macOS shim은 같은 표에서 생성한 `Sources/Transport/AndroidKeyMap.swift`를
//! 쓴다 — 생성물이 이 표와 어긋나면 `tests/parity.rs`의 golden 검사가 잡는다.
//! 사용자 리맵(호스트별 `remoteKeyRemap`)은 이 기본표보다 항상 우선하며,
//! 각 호스트 구현에 그대로 남아 있다.

/// 한 안드로이드 키코드의 플랫폼별 기본 대응. 플랫폼이 기본 대응을 두지
/// 않으면 해당 필드가 `None`이고, 호스트는 그 키를 미매핑으로 기록·폐기한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyMapping {
    pub mac: Option<u16>,
    pub windows: Option<u16>,
    /// Linux evdev 키코드(input-event-codes.h). Wayland 가상 키보드가 이
    /// 값을 그대로 쓰고, XKB 키코드는 evdev + 8이다.
    pub linux: Option<u16>,
}

/// macOS ANSI 가상 키코드(문자 A–Z는 배치가 제각각이라 표로 둔다).
const MAC_LETTERS: [u16; 26] = [
    0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6,
];
const MAC_DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
const MAC_F1_F12: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
/// 키패드 8·9가 91·92로 90을 건너뛴다(kVK_ANSI_Keypad는 연속이 아님).
const MAC_KEYPAD: [u16; 10] = [82, 83, 84, 85, 86, 87, 88, 89, 91, 92];
/// F21+는 Mac·Windows 어느 쪽에도 대응 가상 키가 없어 애초에 범위에 넣지 않는다.
const MAC_F13_F20: [u16; 8] = [105, 107, 113, 106, 64, 79, 80, 90];

/// Linux evdev 키코드. A–Z는 QWERTY 배치상 연속이 아니라 표로 둔다
/// (input-event-codes.h: KEY_A=30 … KEY_Z=44).
const LINUX_LETTERS: [u16; 26] = [
    30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45,
    21, 44,
];
/// 안드로이드 7..=16(KEYCODE_0..9) → evdev 2..=11(KEY_1..KEY_0): 연속.
const LINUX_F1_F12: [u16; 12] = [59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88];
/// 안드로이드 144..=153(NUMPAD_0..9) → evdev KP0,KP1..KP9(연속 아님).
const LINUX_KEYPAD: [u16; 10] = [82, 79, 80, 81, 75, 76, 77, 71, 72, 73];

/// 기본표 조회. 안드로이드 키코드는 `android.view.KeyEvent`의 AKEYCODE_* 값.
pub fn lookup(android: u16) -> Option<KeyMapping> {
    // 연속 범위 팔: 안드로이드 구간이 연속이고 양 플랫폼 대응도 연속인 것들.
    if (29..=54).contains(&android) {
        let i = (android - 29) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_LETTERS[i]),
            windows: Some(0x41 + (android - 29)), // VK_A..VK_Z
            linux: Some(LINUX_LETTERS[i]),
        });
    }
    if (7..=16).contains(&android) {
        let i = (android - 7) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_DIGITS[i]),
            windows: Some(0x30 + (android - 7)), // VK_0..VK_9
            linux: Some(2 + (android - 7)),      // KEY_1..KEY_0
        });
    }
    if (131..=142).contains(&android) {
        let i = (android - 131) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_F1_F12[i]),
            windows: Some(0x70 + (android - 131)), // VK_F1..VK_F12
            linux: Some(LINUX_F1_F12[i]),
        });
    }
    if (144..=153).contains(&android) {
        let i = android - 144;
        return Some(KeyMapping {
            mac: Some(MAC_KEYPAD[i as usize]),
            windows: Some(0x60 + i), // VK_NUMPAD0..VK_NUMPAD9
            linux: Some(LINUX_KEYPAD[i as usize]),
        });
    }
    if (183..=190).contains(&android) {
        // F14 원격 신호 키(태블릿 caps+f 바인딩)는 macOS에서 kVK_F14(107)로
        // 배달하지 않는다. 107은 시스템 밝기 낮춤 단축키와 같은 코드라서,
        // 이 키를 받아 명령으로 소비하는 호스트 도구(KeyBridge 브리지)가
        // 상태를 놓치면 이벤트가 시스템으로 새어 화면이 어두워진다. 시스템
        // 어디에도 바인딩 없는 kVK_F18(79)로 배달해 소비자가 없으면 조용한
        // 키 입력으로 끝나게 한다. Windows VK_F14와 Linux KEY_F14는 충돌이
        // 없어 그대로.
        if android == 184 {
            return Some(KeyMapping {
                mac: Some(79),           // kVK_F18
                windows: Some(0x7C + 1), // VK_F14
                linux: Some(184),        // KEY_F14
            });
        }
        let i = (android - 183) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_F13_F20[i]),
            windows: Some(0x7C + (android - 183)), // VK_F13..VK_F20
            // 안드로이드 F13..F20(183..=190)은 evdev KEY_F13..KEY_F20과 같다.
            linux: Some(183 + (android - 183)),
        });
    }

    // 개별 팔. 왼쪽은 macOS kVK_*, 가운데는 Windows VK_*, 오른쪽은 Linux evdev.
    let (mac, windows, linux) = match android {
        19 => (Some(126), Some(0x26), Some(103)), // DPAD_UP / VK_UP / KEY_UP
        20 => (Some(125), Some(0x28), Some(108)), // DPAD_DOWN / VK_DOWN / KEY_DOWN
        21 => (Some(123), Some(0x25), Some(105)), // DPAD_LEFT / VK_LEFT / KEY_LEFT
        22 => (Some(124), Some(0x27), Some(106)), // DPAD_RIGHT / VK_RIGHT / KEY_RIGHT
        23 => (Some(36), Some(0x0D), Some(28)),   // ENTER / VK_RETURN / KEY_ENTER
        55 => (Some(43), Some(0xBC), Some(51)),   // COMMA / VK_OEM_COMMA / KEY_COMMA
        56 => (Some(47), Some(0xBE), Some(52)),   // PERIOD / VK_OEM_PERIOD / KEY_DOT
        57 => (Some(58), Some(0x12), Some(56)),   // ALT_LEFT / VK_MENU / KEY_LEFTALT
        58 => (Some(61), Some(0xA5), Some(100)),  // ALT_RIGHT / VK_RMENU / KEY_RIGHTALT
        59 => (Some(56), Some(0xA0), Some(42)),   // SHIFT_LEFT / VK_LSHIFT / KEY_LEFTSHIFT
        60 => (Some(60), Some(0xA1), Some(54)),   // SHIFT_RIGHT / VK_RSHIFT / KEY_RIGHTSHIFT
        61 => (Some(48), Some(0x09), Some(15)),   // TAB / VK_TAB / KEY_TAB
        62 => (Some(49), Some(0x20), Some(57)),   // SPACE / VK_SPACE / KEY_SPACE
        66 => (Some(36), Some(0x0D), Some(28)),   // ENTER(키패드 아님) / VK_RETURN / KEY_ENTER
        67 => (Some(51), Some(0x08), Some(14)),   // DEL / VK_BACK / KEY_BACKSPACE
        68 => (Some(50), Some(0xC0), Some(41)),   // GRAVE / VK_OEM_3 / KEY_GRAVE
        69 => (Some(27), Some(0xBD), Some(12)),   // MINUS / VK_OEM_MINUS / KEY_MINUS
        70 => (Some(24), Some(0xBB), Some(13)),   // EQUALS / VK_OEM_PLUS / KEY_EQUAL
        71 => (Some(33), Some(0xDB), Some(26)),   // LEFT_BRACKET / VK_OEM_4 / KEY_LEFTBRACE
        72 => (Some(30), Some(0xDD), Some(27)),   // RIGHT_BRACKET / VK_OEM_6 / KEY_RIGHTBRACE
        73 => (Some(42), Some(0xDC), Some(43)),   // BACKSLASH / VK_OEM_5 / KEY_BACKSLASH
        74 => (Some(41), Some(0xBA), Some(39)),   // SEMICOLON / VK_OEM_1 / KEY_SEMICOLON
        75 => (Some(39), Some(0xDE), Some(40)),   // APOSTROPHE / VK_OEM_7 / KEY_APOSTROPHE
        76 => (Some(44), Some(0xBF), Some(53)),   // SLASH / VK_OEM_2 / KEY_SLASH
        92 => (Some(116), Some(0x21), Some(104)), // PAGE_UP / VK_PRIOR / KEY_PAGEUP
        93 => (Some(121), Some(0x22), Some(109)), // PAGE_DOWN / VK_NEXT / KEY_PAGEDOWN
        111 => (Some(53), Some(0x1B), Some(1)),   // ESCAPE / VK_ESCAPE / KEY_ESC
        112 => (Some(117), Some(0x2E), Some(111)), // FORWARD_DEL / VK_DELETE / KEY_DELETE
        113 => (Some(59), Some(0xA2), Some(29)),  // CTRL_LEFT / VK_LCONTROL / KEY_LEFTCTRL
        114 => (Some(62), Some(0xA3), Some(97)),  // CTRL_RIGHT / VK_RCONTROL / KEY_RIGHTCTRL
        115 => (Some(57), Some(0x14), Some(58)),  // CAPS_LOCK / VK_CAPITAL / KEY_CAPSLOCK
        117 => (Some(55), Some(0x5B), Some(125)), // META_LEFT / VK_LWIN / KEY_LEFTMETA
        118 => (Some(54), Some(0x5C), Some(126)), // META_RIGHT / VK_RWIN / KEY_RIGHTMETA
        122 => (Some(115), Some(0x24), Some(102)), // MOVE_HOME / VK_HOME / KEY_HOME
        123 => (Some(119), Some(0x23), Some(107)), // MOVE_END / VK_END / KEY_END
        124 => (Some(114), Some(0x2D), Some(110)), // INSERT — Mac엔 없어 kVK_Help로 대용 / KEY_INSERT
        143 => (Some(71), Some(0x90), Some(69)),   // NUM_LOCK / VK_NUMLOCK / KEY_NUMLOCK
        154 => (Some(75), Some(0x6F), Some(98)),   // NUMPAD_DIVIDE / VK_DIVIDE / KEY_KPSLASH
        155 => (Some(67), Some(0x6A), Some(55)),   // NUMPAD_MULTIPLY / VK_MULTIPLY / KEY_KPASTERISK
        156 => (Some(78), Some(0x6D), Some(74)),   // NUMPAD_SUBTRACT / VK_SUBTRACT / KEY_KPMINUS
        157 => (Some(69), Some(0x6B), Some(78)),   // NUMPAD_ADD / VK_ADD / KEY_KPPLUS
        158 => (Some(65), Some(0x6E), Some(83)),   // NUMPAD_DOT / VK_DECIMAL / KEY_KPDOT
        159 => (Some(95), Some(0xBC), Some(121)), // NUMPAD_COMMA / VK_OEM_COMMA 재사용 / KEY_KPCOMMA
        160 => (Some(76), Some(0x0D), Some(96)),  // NUMPAD_ENTER / VK_RETURN / KEY_KPENTER
        161 => (Some(81), Some(0xBB), Some(117)), // NUMPAD_EQUALS / VK_OEM_PLUS 대용 / KEY_KPEQUAL
        // 204(한/영 LANGUAGE_SWITCH)는 의도적으로 없다 — 표의 104(kVK_JIS_Kana)
        // 매핑은 한국어 소스에서 아무 일도 하지 않는 잘못된 값이었고, 이 키는
        // shim이 언어 토글 폴백으로 따로 처리한다(native/macos-capture-shim
        // CaptureSession+Input.swift). Linux는 Text 이벤트가 한글을 온전히
        // 전달하므로 언어 토글 없이도 한글 입력이 동작한다.
        _ => (None, None, None),
    };
    if mac.is_none() && windows.is_none() && linux.is_none() {
        return None;
    }
    Some(KeyMapping {
        mac,
        windows,
        linux,
    })
}

/// macOS 기본표만 조회(호스트 주입 경로의 편의 함수).
pub fn mac(android: u16) -> Option<u16> {
    lookup(android)?.mac
}

/// Windows 기본표만 조회(호스트 주입 경로의 편의 함수).
pub fn windows(android: u16) -> Option<u16> {
    lookup(android)?.windows
}

/// Linux evdev 기본표만 조회(호스트 주입 경로의 편의 함수).
pub fn linux(android: u16) -> Option<u16> {
    lookup(android)?.linux
}

/// 생성된 Swift 파일 헤더와 본문. `cargo run -p keymap --bin gen-swift`가 쓴다.
pub fn render_swift() -> String {
    let mut out = String::new();
    out.push_str("// GENERATED FILE — 편집 금지. 단일 소스는 crates/keymap/src/lib.rs 다.\n");
    out.push_str("// 재생성: cargo run -p keymap --bin gen-swift\n");
    out.push_str(
        "// Windows 호스트는 같은 표를 Rust에서 직접 소비한다(golden 검사: keymap tests).\n",
    );
    out.push_str("enum AndroidKeyMap {\n");
    out.push_str("    /// 안드로이드 키코드 → macOS 가상 키코드. 없는 코드는 미매핑.\n");
    out.push_str("    static let mac: [UInt16: UInt16] = [\n");
    for code in 0..=u16::MAX {
        if let Some(mapped) = mac(code) {
            out.push_str(&format!("        {code}: {mapped},\n"));
        }
    }
    out.push_str("    ]\n");
    out.push_str("}\n");
    out
}
