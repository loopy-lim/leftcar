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

/// 기본표 조회. 안드로이드 키코드는 `android.view.KeyEvent`의 AKEYCODE_* 값.
pub fn lookup(android: u16) -> Option<KeyMapping> {
    // 연속 범위 팔: 안드로이드 구간이 연속이고 양 플랫폼 대응도 연속인 것들.
    if (29..=54).contains(&android) {
        let i = (android - 29) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_LETTERS[i]),
            windows: Some(0x41 + (android - 29)), // VK_A..VK_Z
        });
    }
    if (7..=16).contains(&android) {
        let i = (android - 7) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_DIGITS[i]),
            windows: Some(0x30 + (android - 7)), // VK_0..VK_9
        });
    }
    if (131..=142).contains(&android) {
        let i = (android - 131) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_F1_F12[i]),
            windows: Some(0x70 + (android - 131)), // VK_F1..VK_F12
        });
    }
    if (144..=153).contains(&android) {
        let i = android - 144;
        return Some(KeyMapping {
            mac: Some(MAC_KEYPAD[i as usize]),
            windows: Some(0x60 + i), // VK_NUMPAD0..VK_NUMPAD9
        });
    }
    if (183..=190).contains(&android) {
        if android == 184 {
            // F14 원격 신호 키(태블릿 caps+f 바인딩)는 macOS에서 kVK_F14(107)로
            // 배달하지 않는다. 107은 시스템 밝기 낮춤 단축키와 같은 코드라서,
            // 이 키를 받아 명령으로 소비하는 호스트 도구(KeyBridge 브리지)가
            // 상태를 놓치면 이벤트가 시스템으로 새어 화면이 어두워진다. 시스템
            // 어디에도 바인딩 없는 kVK_F18(79)로 배달해 소비자가 없으면 조용한
            // 키 입력으로 끝나게 한다. Windows VK_F14는 충돌이 없어 그대로.
            return Some(KeyMapping {
                mac: Some(79),           // kVK_F18
                windows: Some(0x7C + 1), // VK_F14
            });
        }
        let i = (android - 183) as usize;
        return Some(KeyMapping {
            mac: Some(MAC_F13_F20[i]),
            windows: Some(0x7C + (android - 183)), // VK_F13..VK_F20
        });
    }

    // 개별 팔. 왼쪽은 macOS kVK_*, 오른쪽은 Windows VK_*.
    let (mac, windows) = match android {
        19 => (Some(126), Some(0x26)),  // DPAD_UP / VK_UP
        20 => (Some(125), Some(0x28)),  // DPAD_DOWN / VK_DOWN
        21 => (Some(123), Some(0x25)),  // DPAD_LEFT / VK_LEFT
        22 => (Some(124), Some(0x27)),  // DPAD_RIGHT / VK_RIGHT
        23 => (Some(36), Some(0x0D)),   // ENTER / VK_RETURN
        55 => (Some(43), Some(0xBC)),   // COMMA / VK_OEM_COMMA
        56 => (Some(47), Some(0xBE)),   // PERIOD / VK_OEM_PERIOD
        57 => (Some(58), Some(0x12)),   // ALT_LEFT / VK_MENU
        58 => (Some(61), Some(0xA5)),   // ALT_RIGHT / VK_RMENU
        59 => (Some(56), Some(0xA0)),   // SHIFT_LEFT / VK_LSHIFT
        60 => (Some(60), Some(0xA1)),   // SHIFT_RIGHT / VK_RSHIFT
        61 => (Some(48), Some(0x09)),   // TAB / VK_TAB
        62 => (Some(49), Some(0x20)),   // SPACE / VK_SPACE
        66 => (Some(36), Some(0x0D)),   // ENTER(키패드 아님) / VK_RETURN
        67 => (Some(51), Some(0x08)),   // DEL / VK_BACK
        68 => (Some(50), Some(0xC0)),   // GRAVE / VK_OEM_3
        69 => (Some(27), Some(0xBD)),   // MINUS / VK_OEM_MINUS
        70 => (Some(24), Some(0xBB)),   // EQUALS / VK_OEM_PLUS
        71 => (Some(33), Some(0xDB)),   // LEFT_BRACKET / VK_OEM_4
        72 => (Some(30), Some(0xDD)),   // RIGHT_BRACKET / VK_OEM_6
        73 => (Some(42), Some(0xDC)),   // BACKSLASH / VK_OEM_5
        74 => (Some(41), Some(0xBA)),   // SEMICOLON / VK_OEM_1
        75 => (Some(39), Some(0xDE)),   // APOSTROPHE / VK_OEM_7
        76 => (Some(44), Some(0xBF)),   // SLASH / VK_OEM_2
        92 => (Some(116), Some(0x21)),  // PAGE_UP / VK_PRIOR
        93 => (Some(121), Some(0x22)),  // PAGE_DOWN / VK_NEXT
        111 => (Some(53), Some(0x1B)),  // ESCAPE / VK_ESCAPE
        112 => (Some(117), Some(0x2E)), // FORWARD_DEL / VK_DELETE
        113 => (Some(59), Some(0xA2)),  // CTRL_LEFT / VK_LCONTROL
        114 => (Some(62), Some(0xA3)),  // CTRL_RIGHT / VK_RCONTROL
        115 => (Some(57), Some(0x14)),  // CAPS_LOCK / VK_CAPITAL
        117 => (Some(55), Some(0x5B)),  // META_LEFT / VK_LWIN
        118 => (Some(54), Some(0x5C)),  // META_RIGHT / VK_RWIN
        122 => (Some(115), Some(0x24)), // MOVE_HOME / VK_HOME
        123 => (Some(119), Some(0x23)), // MOVE_END / VK_END
        124 => (Some(114), Some(0x2D)), // INSERT — Mac엔 없어 kVK_Help로 대용
        143 => (Some(71), Some(0x90)),  // NUM_LOCK / VK_NUMLOCK
        154 => (Some(75), Some(0x6F)),  // NUMPAD_DIVIDE / VK_DIVIDE
        155 => (Some(67), Some(0x6A)),  // NUMPAD_MULTIPLY / VK_MULTIPLY
        156 => (Some(78), Some(0x6D)),  // NUMPAD_SUBTRACT / VK_SUBTRACT
        157 => (Some(69), Some(0x6B)),  // NUMPAD_ADD / VK_ADD
        158 => (Some(65), Some(0x6E)),  // NUMPAD_DOT / VK_DECIMAL
        159 => (Some(95), Some(0xBC)),  // NUMPAD_COMMA / Windows는 VK_OEM_COMMA 재사용
        160 => (Some(76), Some(0x0D)),  // NUMPAD_ENTER / VK_RETURN
        161 => (Some(81), Some(0xBB)),  // NUMPAD_EQUALS / Windows는 VK_OEM_PLUS로 대용
        // 204(한/영 LANGUAGE_SWITCH)는 의도적으로 없다 — 표의 104(kVK_JIS_Kana)
        // 매핑은 한국어 소스에서 아무 일도 하지 않는 잘못된 값이었고, 이 키는
        // shim이 언어 토글 폴백으로 따로 처리한다(native/macos-capture-shim
        // CaptureSession+Input.swift).
        _ => (None, None),
    };
    if mac.is_none() && windows.is_none() {
        return None;
    }
    Some(KeyMapping { mac, windows })
}

/// macOS 기본표만 조회(호스트 주입 경로의 편의 함수).
pub fn mac(android: u16) -> Option<u16> {
    lookup(android)?.mac
}

/// Windows 기본표만 조회(호스트 주입 경로의 편의 함수).
pub fn windows(android: u16) -> Option<u16> {
    lookup(android)?.windows
}

/// 생성된 Swift 파일 헤더와 본문. `cargo run -p keymap --bin gen-swift`가 쓴다.
pub fn render_swift() -> String {
    let mut out = String::new();
    out.push_str("// GENERATED FILE — 편집 금지. 단일 소스는 crates/keymap/src/lib.rs 다.\n");
    out.push_str("// 재생성: cargo run -p keymap --bin gen-swift\n");
    out.push_str("// Windows 호스트는 같은 표를 Rust에서 직접 소비한다(golden 검사: keymap tests).\n");
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
