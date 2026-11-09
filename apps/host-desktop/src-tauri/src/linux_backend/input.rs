//! Per-session remote input injection onto wlr virtual devices. Mirrors
//! `windows_backend/input.rs`: it remembers every injected down transition so
//! disabling Control, disconnecting, or stopping can always synthesize ups.
//!
//! Text events use dynamically assigned xkb keycodes (the wtype approach):
//! each committed character gets a keysym bound to a reserved keycode in the
//! uploaded keymap, so layout-independent text (Hangul, emoji) arrives as
//! typed without a host-side IME.

use std::collections::HashSet;

use super::wayland::VirtualInput;
use crate::wire::InputEvent;

// evdev button codes (input-event-codes.h).
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113; // Android BUTTON_BACK
const BTN_EXTRA: u32 = 0x114; // Android BUTTON_FORWARD

// evdev modifier keys used for the wire's Android meta_state bits.
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTALT: u16 = 56;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTMETA: u16 = 125;

// Android KeyEvent META_* flags this injector honors.
const META_SHIFT: u32 = 0x1;
const META_ALT: u32 = 0x2;
const META_CTRL: u32 = 0x1000;
const META_META: u32 = 0x40000;

/// Committed text rides reserved xkb keycodes 248..=254 (XKB keycodes stop at
/// 255; evdev = xkb − 8, so these are evdev 240..=246). Seven concurrent
/// distinct characters before the oldest slot is recycled and the keymap is
/// re-uploaded — fine for committed IME chunks, and the documented v1 limit.
const DYNAMIC_XKB_BASE: u16 = 248;
const DYNAMIC_SLOTS: usize = 7;

pub(crate) struct InputInjector {
    devices: VirtualInput,
    /// Captured output's top-left corner relative to the layout origin —
    /// the viewer normalizes over the captured output only.
    output_rel_x: i32,
    output_rel_y: i32,
    output_w: u32,
    output_h: u32,
    /// Global layout size (union of outputs) for the absolute extents.
    layout_w: u32,
    layout_h: u32,
    pressed_keys: HashSet<u16>,
    pressed_buttons: HashSet<u8>,
    held_modifiers: HashSet<u16>,
    horizontal_remainder: i32,
    vertical_remainder: i32,
    /// 미매핑 안드로이드 키코드 진단 로그 중복 방지(키코드당 1줄) — Windows와
    /// 같은 진단 규칙.
    logged_unmapped: HashSet<u16>,
    keymap: TextKeymap,
}

impl InputInjector {
    pub(crate) fn new(
        devices: VirtualInput,
        output_rel_x: i32,
        output_rel_y: i32,
        output_w: u32,
        output_h: u32,
        layout_w: u32,
        layout_h: u32,
    ) -> Result<Self, String> {
        let mut injector = Self {
            devices,
            output_rel_x,
            output_rel_y,
            output_w: output_w.max(1),
            output_h: output_h.max(1),
            layout_w: layout_w.max(1),
            layout_h: layout_h.max(1),
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
            held_modifiers: HashSet::new(),
            horizontal_remainder: 0,
            vertical_remainder: 0,
            logged_unmapped: HashSet::new(),
            keymap: TextKeymap::default(),
        };
        injector.devices.set_keymap(&injector.keymap.render())?;
        Ok(injector)
    }

    pub(crate) fn apply(&mut self, event: InputEvent) -> Result<(), String> {
        match event {
            // 스타일러스 압력은 디코딩되지만 Wayland 가상 포인터 주입 경로에는
            // 압력 필드가 없어 무시된다 — macOS shim만 태블릿 서브타입으로
            // 반영한다(Windows와 같은 v1 규칙).
            InputEvent::PointerMove { x, y, .. } => self.move_pointer(x, y),
            InputEvent::PointerButton {
                x, y, button, down, ..
            } => {
                self.move_pointer(x, y)?;
                self.button(button, down)
            }
            InputEvent::Scroll {
                horizontal_milli,
                vertical_milli,
            } => self.scroll(horizontal_milli, vertical_milli),
            InputEvent::Key {
                key_code,
                meta_state,
                down,
                ..
            } => self.key(key_code, meta_state, down),
            InputEvent::Text { text } => self.text(&text),
            InputEvent::ReleaseAll => self.release_all(),
        }
    }

    pub(crate) fn release_all(&mut self) -> Result<(), String> {
        let keys = self.pressed_keys.drain().collect::<Vec<_>>();
        for evdev in keys {
            self.devices.key(evdev, false)?;
        }
        self.held_modifiers.clear();
        let buttons = self.pressed_buttons.drain().collect::<Vec<_>>();
        for button in buttons {
            self.button_code(button, false)?;
        }
        self.horizontal_remainder = 0;
        self.vertical_remainder = 0;
        Ok(())
    }

    fn move_pointer(&mut self, x: u16, y: u16) -> Result<(), String> {
        // The viewer normalizes over the captured output's content rect; map
        // onto the compositor's global output layout by adding the captured
        // output's offset within that layout (the virtual-desktop equivalent
        // of the Windows injector, which subtracts SM_XVIRTUALSCREEN).
        let absolute_x = (self.output_rel_x as i64
            + i64::from(x) * (self.output_w as i64 - 1) / 65_535)
        .clamp(0, self.layout_w as i64 - 1) as u32;
        let absolute_y = (self.output_rel_y as i64
            + i64::from(y) * (self.output_h as i64 - 1) / 65_535)
        .clamp(0, self.layout_h as i64 - 1) as u32;
        self.devices
            .move_absolute(absolute_x, absolute_y, self.layout_w, self.layout_h)
    }

    fn button(&mut self, button: u8, down: bool) -> Result<(), String> {
        self.button_code(button, down)?;
        if down {
            self.pressed_buttons.insert(button);
        } else {
            self.pressed_buttons.remove(&button);
        }
        Ok(())
    }

    fn button_code(&mut self, button: u8, down: bool) -> Result<(), String> {
        let code = match button {
            1 => BTN_LEFT,
            2 => BTN_RIGHT,
            4 => BTN_MIDDLE,
            8 => BTN_SIDE,
            16 => BTN_EXTRA,
            other => return Err(format!("unsupported mouse button mask {other}")),
        };
        self.devices.button(code, down)
    }

    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<(), String> {
        self.horizontal_remainder = self.horizontal_remainder.saturating_add(horizontal);
        self.vertical_remainder = self.vertical_remainder.saturating_add(vertical);
        let horizontal_steps = self.horizontal_remainder / 1_000;
        let vertical_steps = self.vertical_remainder / 1_000;
        self.horizontal_remainder %= 1_000;
        self.vertical_remainder %= 1_000;
        if vertical_steps != 0 || horizontal_steps != 0 {
            self.devices
                .wheel(f64::from(vertical_steps), f64::from(horizontal_steps))?;
        }
        Ok(())
    }

    fn key(&mut self, android_key_code: u16, meta_state: u32, down: bool) -> Result<(), String> {
        // 기본표의 단일 소스는 crates/keymap — macOS shim/Windows 호스트는 같은
        // 표의 각자 열을 쓴다.
        let Some(evdev) = keymap::linux(android_key_code) else {
            // 조용히 버리면 뷰어가 원인 없이 "안 쳐진다"를 겪는다 — 다른
            // 플랫폼과 같은 진단 로그를 키코드당 한 번만 남긴다.
            if self.logged_unmapped.insert(android_key_code) {
                eprintln!("leftcar: unmapped android keycode {android_key_code} dropped (Linux)");
            }
            return Ok(());
        };
        if down {
            self.press_modifiers(meta_state)?;
            self.devices.key(evdev, true)?;
            self.pressed_keys.insert(evdev);
        } else {
            self.devices.key(evdev, false)?;
            self.pressed_keys.remove(&evdev);
            self.release_modifiers_when_idle()?;
        }
        Ok(())
    }

    /// Modifier keys are plain evdev keys to the compositor; press them before
    /// the key and release once no regular key is held anymore.
    fn press_modifiers(&mut self, meta_state: u32) -> Result<(), String> {
        let mut wanted = Vec::new();
        if meta_state & META_SHIFT != 0 {
            wanted.push(KEY_LEFTSHIFT);
        }
        if meta_state & META_ALT != 0 {
            wanted.push(KEY_LEFTALT);
        }
        if meta_state & META_CTRL != 0 {
            wanted.push(KEY_LEFTCTRL);
        }
        if meta_state & META_META != 0 {
            wanted.push(KEY_LEFTMETA);
        }
        for modifier in wanted {
            if self.held_modifiers.insert(modifier) {
                self.devices.key(modifier, true)?;
                self.pressed_keys.insert(modifier);
            }
        }
        Ok(())
    }

    fn release_modifiers_when_idle(&mut self) -> Result<(), String> {
        let regular_held = self
            .pressed_keys
            .iter()
            .any(|key| !self.held_modifiers.contains(key));
        if regular_held {
            return Ok(());
        }
        let modifiers = self.held_modifiers.drain().collect::<Vec<_>>();
        for modifier in modifiers {
            self.pressed_keys.remove(&modifier);
            self.devices.key(modifier, false)?;
        }
        Ok(())
    }

    /// Committed IME text. Each character rides a dynamic xkb keycode bound to
    /// its unicode keysym, so Hangul/emoji arrive as typed without a host
    /// IME. No key state is retained: each character is a complete down+up
    /// pair (same rule as the Windows KEYEVENTF_UNICODE path).
    fn text(&mut self, text: &str) -> Result<(), String> {
        // Resolve every character first: a newly bound keycode must exist in
        // the keymap the compositor holds BEFORE the press arrives, so a
        // changed map is uploaded up front (wtype's ordering).
        let mut presses = Vec::new();
        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            if let Some(evdev) = self.keymap.slot_for(ch) {
                presses.push(evdev);
            }
        }
        if self.keymap.dirty() {
            let rendered = self.keymap.render();
            self.devices.set_keymap(&rendered)?;
            self.keymap.clean();
        }
        for evdev in presses {
            self.devices.key(evdev, true)?;
            self.devices.key(evdev, false)?;
        }
        Ok(())
    }
}

impl Drop for InputInjector {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

/// Dynamic keycode assignment for text injection. Slot order is rotated FIFO
/// so a stream of distinct characters re-uses the oldest binding and only
/// re-uploads the keymap when the assignment actually changed.
#[derive(Default)]
struct TextKeymap {
    slots: [Option<(char, u32)>; DYNAMIC_SLOTS],
    next_slot: usize,
    changed: bool,
}

impl TextKeymap {
    /// Reserve (or reuse) a slot for `ch`; returns the evdev keycode.
    fn slot_for(&mut self, ch: char) -> Option<u16> {
        let keysym = keysym_value(ch)?;
        if let Some((index, _)) = self
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| **slot == Some((ch, keysym)))
        {
            return Some(DYNAMIC_XKB_BASE - 8 + index as u16);
        }
        let index = self.next_slot % DYNAMIC_SLOTS;
        if self.slots[index].is_some() {
            // Rebinding invalidates the whole map for the client only if the
            // keysym differs; either way mark dirty and re-upload once.
            self.changed = true;
        } else {
            self.changed = true;
        }
        self.slots[index] = Some((ch, keysym));
        self.next_slot = (index + 1) % DYNAMIC_SLOTS;
        Some(DYNAMIC_XKB_BASE - 8 + index as u16)
    }

    fn dirty(&self) -> bool {
        self.changed
    }

    fn clean(&mut self) {
        self.changed = false;
    }

    /// Full xkb keymap text: the standard evdev keycode table with a US
    /// layout for regular keys, plus our dynamic slots appended. Includes
    /// resolve against the receiving client's xkeyboard-config, exactly like
    /// wtype's generated maps.
    fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("xkb_keymap {\n");
        out.push_str("    xkb_keycodes {\n");
        out.push_str("        include \"evdev+aliases(qwerty)\"\n");
        for index in 0..DYNAMIC_SLOTS {
            out.push_str(&format!(
                "        <LC{index:02}> = {};\n",
                DYNAMIC_XKB_BASE + index as u16
            ));
        }
        out.push_str("    };\n");
        out.push_str("    xkb_types { include \"complete\" };\n");
        out.push_str("    xkb_compat { include \"complete\" };\n");
        out.push_str("    xkb_symbols {\n");
        out.push_str("        include \"pc+us\"\n");
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some((_, keysym)) = slot {
                out.push_str(&format!(
                    "        key <LC{index:02}> {{ [ {} ] }};\n",
                    keysym_name(*keysym)
                ));
            }
        }
        out.push_str("    };\n");
        out.push_str("    xkb_geometry { include \"pc(pc105)\" };\n");
        out.push_str("};\n");
        out
    }
}

/// Unicode keysym value for a character, or None for control characters.
/// Latin-1 printable characters are their own keysyms; everything above uses
/// the 0x01000000 + codepoint range.
fn keysym_value(ch: char) -> Option<u32> {
    let code = u32::from(ch);
    if code < 0x20 {
        return None;
    }
    if code < 0x100 {
        return Some(code);
    }
    Some(0x0100_0000 + code)
}

/// xkb symbol-file name for a keysym. Latin-1 printable characters use their
/// standard names; codepoints ≥ 0x100 use the `UXXXX` unicode keysym name.
fn keysym_name(keysym: u32) -> String {
    if keysym < 0x100 {
        let ch = char::from_u32(keysym).expect("latin-1 keysyms are valid chars");
        if let Some(name) = latin1_name(ch) {
            return name.to_owned();
        }
        return ch.to_string();
    }
    let code = keysym - 0x0100_0000;
    // libxkbcommon keysym names are case-sensitive: only the uppercase
    // `UXXXX` spelling resolves (lowercase `uXXXX` silently parses as
    // NoSymbol), and that is the xkeyboard-config convention.
    format!("U{code:04X}")
}

fn latin1_name(ch: char) -> Option<&'static str> {
    Some(match ch {
        ' ' => "space",
        '!' => "exclam",
        '"' => "quotedbl",
        '#' => "numbersign",
        '$' => "dollar",
        '%' => "percent",
        '&' => "ampersand",
        '\'' => "apostrophe",
        '(' => "parenleft",
        ')' => "parenright",
        '*' => "asterisk",
        '+' => "plus",
        ',' => "comma",
        '-' => "minus",
        '.' => "period",
        '/' => "slash",
        ':' => "colon",
        ';' => "semicolon",
        '<' => "less",
        '=' => "equal",
        '>' => "greater",
        '?' => "question",
        '@' => "at",
        '[' => "bracketleft",
        '\\' => "backslash",
        ']' => "bracketright",
        '^' => "asciicircum",
        '_' => "underscore",
        '`' => "grave",
        '{' => "braceleft",
        '|' => "bar",
        '}' => "braceright",
        '~' => "asciitilde",
        // Alphanumerics render as themselves (handled by the caller).
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_text_contains_dynamic_slots() {
        let mut keymap = TextKeymap::default();
        let evdev = keymap.slot_for('가').expect("hangul char gets a slot");
        assert_eq!(evdev, DYNAMIC_XKB_BASE - 8);
        let text = keymap.render();
        assert!(text.contains("= 248;"), "keycodes section: {text}");
        assert!(text.contains("key <LC00> { [ UAC00 ] };"), "symbols: {text}");
        assert!(text.contains("include \"pc+us\""));
        assert!(keymap.dirty());
    }

    #[test]
    fn reusing_a_slot_recycles_oldest_and_renames() {
        let mut keymap = TextKeymap::default();
        keymap.slot_for('a').expect("first slot");
        for ch in ['b', 'c', 'd', 'e', 'f', 'g'] {
            keymap.slot_for(ch).expect("slot");
        }
        // 8th distinct character recycles slot 0.
        let evdev = keymap.slot_for('h').expect("recycled slot");
        assert_eq!(evdev, DYNAMIC_XKB_BASE - 8);
        assert!(keymap.render().contains("key <LC00> { [ h ] };"));
        keymap.clean();
        // Same character again: no new assignment, no churn.
        assert_eq!(keymap.slot_for('h'), Some(DYNAMIC_XKB_BASE - 8));
        assert!(!keymap.dirty());
    }

    #[test]
    fn latin1_characters_map_to_named_keysyms() {
        assert_eq!(keysym_name(keysym_value('!').unwrap()), "exclam");
        assert_eq!(keysym_name(keysym_value('z').unwrap()), "z");
        assert_eq!(keysym_value('z').unwrap(), 0x7A);
        assert_eq!(keysym_name(keysym_value('가').unwrap()), "UAC00");
        assert_eq!(keysym_value('\n'), None);
    }
}
