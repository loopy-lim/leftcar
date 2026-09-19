use crate::wire::InputEvent;
use std::collections::HashSet;
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Per-session SendInput state. It remembers every injected down transition so
/// disabling Control, disconnecting, or stopping can always synthesize ups.
pub struct InputInjector {
    monitor: RECT,
    pressed_keys: HashSet<u16>,
    pressed_buttons: HashSet<u8>,
    horizontal_remainder: i32,
    vertical_remainder: i32,
    /// 미매핑 안드로이드 키코드 진단 로그 중복 방지(키코드당 1줄).
    logged_unmapped: HashSet<u16>,
}

impl InputInjector {
    pub fn new(monitor: RECT) -> Self {
        Self {
            monitor,
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
            horizontal_remainder: 0,
            vertical_remainder: 0,
            logged_unmapped: HashSet::new(),
        }
    }

    pub fn apply(&mut self, event: InputEvent) -> Result<(), String> {
        match event {
            // 스타일러스 압력은 디코딩되지만 v1 Windows 주입 경로(SendInput
            // 마우스)에는 압력 필드가 없어 무시된다 — macOS shim만
            // 태블릿 서브타입으로 반영한다.
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
            InputEvent::Key { key_code, down, .. } => self.key(key_code, down),
            InputEvent::Text { text } => self.text(&text),
            InputEvent::ReleaseAll => self.release_all(),
        }
    }

    pub fn release_all(&mut self) -> Result<(), String> {
        let keys = self.pressed_keys.drain().collect::<Vec<_>>();
        for virtual_key in keys {
            send_keyboard(virtual_key, false)?;
        }
        let buttons = self.pressed_buttons.drain().collect::<Vec<_>>();
        for button in buttons {
            send_mouse(0, 0, 0, button_flag(button, false)?)?;
        }
        self.horizontal_remainder = 0;
        self.vertical_remainder = 0;
        Ok(())
    }

    fn move_pointer(&self, x: u16, y: u16) -> Result<(), String> {
        let monitor_width = (self.monitor.right - self.monitor.left).max(1) as i64;
        let monitor_height = (self.monitor.bottom - self.monitor.top).max(1) as i64;
        let pixel_x = self.monitor.left as i64 + i64::from(x) * (monitor_width - 1) / 65_535;
        let pixel_y = self.monitor.top as i64 + i64::from(y) * (monitor_height - 1) / 65_535;
        let virtual_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as i64;
        let virtual_top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as i64;
        let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as i64;
        let virtual_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as i64;
        let absolute_x = ((pixel_x - virtual_left) * 65_535 / (virtual_width - 1).max(1))
            .clamp(0, 65_535) as i32;
        let absolute_y = ((pixel_y - virtual_top) * 65_535 / (virtual_height - 1).max(1))
            .clamp(0, 65_535) as i32;
        send_mouse(
            absolute_x,
            absolute_y,
            0,
            MOUSEEVENTF_MOVE
                | MOUSEEVENTF_ABSOLUTE
                | MOUSEEVENTF_VIRTUALDESK
                | MOUSEEVENTF_MOVE_NOCOALESCE,
        )
    }

    fn button(&mut self, button: u8, down: bool) -> Result<(), String> {
        if button == 8 || button == 16 {
            // Android BUTTON_BACK/FORWARD: XBUTTON1 = back, XBUTTON2 =
            // forward. X-button injection carries the button id in mouseData
            // instead of a per-button flag.
            let data: u32 = if button == 8 { 0x0001 } else { 0x0002 };
            let flags = if down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP };
            send_mouse(0, 0, data, flags)?;
        } else {
            send_mouse(0, 0, 0, button_flag(button, down)?)?;
        }
        if down {
            self.pressed_buttons.insert(button);
        } else {
            self.pressed_buttons.remove(&button);
        }
        Ok(())
    }

    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<(), String> {
        self.horizontal_remainder = self.horizontal_remainder.saturating_add(horizontal);
        self.vertical_remainder = self.vertical_remainder.saturating_add(vertical);
        let horizontal_steps = self.horizontal_remainder / 1_000;
        let vertical_steps = self.vertical_remainder / 1_000;
        self.horizontal_remainder %= 1_000;
        self.vertical_remainder %= 1_000;
        if vertical_steps != 0 {
            send_mouse(0, 0, (vertical_steps * 120) as u32, MOUSEEVENTF_WHEEL)?;
        }
        if horizontal_steps != 0 {
            send_mouse(0, 0, (horizontal_steps * 120) as u32, MOUSEEVENTF_HWHEEL)?;
        }
        Ok(())
    }

    fn key(&mut self, android_key_code: u16, down: bool) -> Result<(), String> {
        // 기본표의 단일 소스는 crates/keymap — macOS shim은 같은 표의
        // 생성물(AndroidKeyMap.swift)을 쓴다.
        let Some(virtual_key) = keymap::windows(android_key_code) else {
            // 조용히 버리면 뷰어가 원인 없이 "안 쳐진다"를 겪는다 — macOS shim과
            // 같은 진단 로그를 키코드당 한 번만 남긴다.
            if self.logged_unmapped.insert(android_key_code) {
                eprintln!("leftcar: unmapped android keycode {android_key_code} dropped (Windows)");
            }
            return Ok(());
        };
        send_keyboard(virtual_key, down)?;
        if down {
            self.pressed_keys.insert(virtual_key);
        } else {
            self.pressed_keys.remove(&virtual_key);
        }
        Ok(())
    }

    /// Committed IME text. KEYEVENTF_UNICODE types the exact UTF-16 units, so
    /// layout-independent text (Hangul, emoji, surrogate pairs) arrives as
    /// typed without synthesizing per-key events. No key state is retained:
    /// each unit is a complete down+up pair.
    fn text(&mut self, text: &str) -> Result<(), String> {
        let inputs: Vec<INPUT> = text
            .encode_utf16()
            .flat_map(|unit| [unicode_input(unit, false), unicode_input(unit, true)])
            .collect();
        if inputs.is_empty() {
            return Ok(());
        }
        send(&inputs)
    }
}

impl Drop for InputInjector {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

fn send_mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> Result<(), String> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[input])
}

fn send_keyboard(virtual_key: u16, down: bool) -> Result<(), String> {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[input])
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(format!(
            "SendInput injected {sent}/{} events; Windows UIPI may be blocking a higher-integrity target",
            inputs.len()
        ))
    }
}

fn button_flag(button: u8, down: bool) -> Result<MOUSE_EVENT_FLAGS, String> {
    match (button, down) {
        (1, true) => Ok(MOUSEEVENTF_LEFTDOWN),
        (1, false) => Ok(MOUSEEVENTF_LEFTUP),
        (2, true) => Ok(MOUSEEVENTF_RIGHTDOWN),
        (2, false) => Ok(MOUSEEVENTF_RIGHTUP),
        (4, true) => Ok(MOUSEEVENTF_MIDDLEDOWN),
        (4, false) => Ok(MOUSEEVENTF_MIDDLEUP),
        _ => Err(format!("unsupported mouse button mask {button}")),
    }
}
