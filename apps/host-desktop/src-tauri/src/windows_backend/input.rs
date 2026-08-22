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
}

impl InputInjector {
    pub fn new(monitor: RECT) -> Self {
        Self {
            monitor,
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
            horizontal_remainder: 0,
            vertical_remainder: 0,
        }
    }

    pub fn apply(&mut self, event: InputEvent) -> Result<(), String> {
        match event {
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
        send_mouse(0, 0, 0, button_flag(button, down)?)?;
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
        let Some(virtual_key) = android_to_virtual_key(android_key_code) else {
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

fn android_to_virtual_key(code: u16) -> Option<u16> {
    if (29..=54).contains(&code) {
        return Some(VK_A.0 + code - 29);
    }
    if (7..=16).contains(&code) {
        return Some(VK_0.0 + code - 7);
    }
    if (131..=142).contains(&code) {
        return Some(VK_F1.0 + code - 131);
    }
    if (144..=153).contains(&code) {
        return Some(VK_NUMPAD0.0 + code - 144);
    }
    Some(match code {
        19 => VK_UP.0,
        20 => VK_DOWN.0,
        21 => VK_LEFT.0,
        22 => VK_RIGHT.0,
        55 => VK_OEM_COMMA.0,
        56 => VK_OEM_PERIOD.0,
        57 => VK_MENU.0,
        58 => VK_RMENU.0,
        59 => VK_LSHIFT.0,
        60 => VK_RSHIFT.0,
        61 => VK_TAB.0,
        62 => VK_SPACE.0,
        66 => VK_RETURN.0,
        67 => VK_BACK.0,
        68 => VK_OEM_3.0,
        69 => VK_OEM_MINUS.0,
        70 => VK_OEM_PLUS.0,
        71 => VK_OEM_4.0,
        72 => VK_OEM_6.0,
        73 => VK_OEM_5.0,
        74 => VK_OEM_1.0,
        75 => VK_OEM_7.0,
        76 => VK_OEM_2.0,
        92 => VK_PRIOR.0,
        93 => VK_NEXT.0,
        111 => VK_ESCAPE.0,
        112 => VK_DELETE.0,
        113 => VK_LCONTROL.0,
        114 => VK_RCONTROL.0,
        115 => VK_CAPITAL.0,
        117 => VK_LWIN.0,
        118 => VK_RWIN.0,
        122 => VK_HOME.0,
        123 => VK_END.0,
        124 => VK_INSERT.0,
        154 => VK_DIVIDE.0,
        155 => VK_MULTIPLY.0,
        156 => VK_SUBTRACT.0,
        157 => VK_ADD.0,
        158 => VK_DECIMAL.0,
        160 => VK_RETURN.0,
        161 => VK_OEM_PLUS.0,
        _ => return None,
    })
}
