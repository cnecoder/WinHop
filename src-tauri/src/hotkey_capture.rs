//! 全局热键录制：Rust 侧轮询 GetAsyncKeyState 检测组合键。
//! webview 事件会被中文输入法吞掉（Ctrl+Space 的 keydown 被 IME 用于切中英），
//! 物理键状态 GetAsyncKeyState 不受影响——录制改走轮询，绕开事件系统。
//! 本子系统自包含：不依赖 Inner/状态机，仅通过命令与前端交互（start/poll/stop）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static CAPTURE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CAPTURE_ON: AtomicBool = AtomicBool::new(false);

// (虚拟键码, 组合串名)。左右 Ctrl 归一为 ctrl。
const VK_MODS: [(i32, &'static str); 6] = [
    (0x11, "ctrl"),
    (0xA2, "ctrl"),
    (0xA3, "ctrl"),
    (0x12, "alt"),
    (0x10, "shift"),
    (0x5B, "super"),
];

fn mods_down() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (vk, name) in VK_MODS {
        if key_down(vk) && !out.iter().any(|n| *n == name) {
            out.push(name);
        }
    }
    // 固定顺序：ctrl alt shift super
    let order = ["ctrl", "alt", "shift", "super"];
    out.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(99));
    out
}

// 主键 vk → 组合串里的键名；None 表示该 vk 是修饰键
pub(crate) fn vk_key_name(vk: i32) -> Option<String> {
    match vk {
        0x20 => Some("space".into()),
        0x41..=0x5A => Some(((b'a' + (vk - 0x41) as u8) as char).to_string()),
        0x30..=0x39 => Some(((b'0' + (vk - 0x30) as u8) as char).to_string()),
        0x70..=0x87 => Some(format!("f{}", vk - 0x70 + 1)),
        _ => None,
    }
}

fn key_down(vk: i32) -> bool {
    unsafe {
        (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as i32 & 0x8000)
            != 0
    }
}

// 命中组合：mods + 主键 vk → 记录并停线程；返回 true 表示已命中
fn capture_hit(mods: &[&'static str], vk: i32) -> bool {
    if let Some(name) = vk_key_name(vk) {
        let mut combo = mods.to_vec();
        combo.push(name.as_str());
        if let Some(slot) = CAPTURE.get() {
            *slot.lock().unwrap() = Some(combo.join("+"));
        }
        CAPTURE_ON.store(false, Ordering::Relaxed);
        return true;
    }
    false
}

// 开始检测：后台线程轮询，主键「按下沿」+ 修饰键按住 → 记录组合，一次后停止
#[tauri::command]
pub(crate) fn hotkey_capture_start() {
    CAPTURE.get_or_init(|| Mutex::new(None));
    CAPTURE_ON.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        let mut prev: HashSet<i32> = HashSet::new();
        let mut prev_mods: Vec<&'static str> = Vec::new();
        while CAPTURE_ON.load(Ordering::Relaxed) {
            let mods = mods_down();
            let mut now: HashSet<i32> = HashSet::new();
            for vk in 0x41..=0x5A {
                if key_down(vk) {
                    now.insert(vk);
                }
            } // A-Z
            for vk in 0x30..=0x39 {
                if key_down(vk) {
                    now.insert(vk);
                }
            } // 0-9
            for vk in 0x70..=0x87 {
                if key_down(vk) {
                    now.insert(vk);
                }
            } // F1-F24
            if key_down(0x20) {
                now.insert(0x20);
            } // Space
            // 方向 1：主键按下沿（上一轮未按、本轮按下）且修饰键已按住
            for &vk in now.difference(&prev) {
                if mods.is_empty() {
                    break;
                }
                if capture_hit(&mods, vk) {
                    return;
                }
            }
            // 方向 2：修饰键刚按下（按下沿）且已有主键按住（先按主键/同时按）
            if !mods.is_empty() && mods.iter().any(|m| !prev_mods.contains(m)) && !now.is_empty() {
                let vk = *now.iter().next().unwrap();
                if capture_hit(&mods, vk) {
                    return;
                }
            }
            prev = now;
            prev_mods = mods;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    });
}

#[tauri::command]
pub(crate) fn hotkey_capture_poll() -> Option<String> {
    CAPTURE.get().and_then(|m| m.lock().unwrap().take())
}

#[tauri::command]
pub(crate) fn hotkey_capture_stop() {
    CAPTURE_ON.store(false, Ordering::Relaxed);
    if let Some(m) = CAPTURE.get() {
        *m.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_key_name_maps_keys() {
        // 空格 / 字母 / 数字 / 功能键 → 组合串名；修饰键等返回 None
        assert_eq!(vk_key_name(0x20).as_deref(), Some("space"));
        assert_eq!(vk_key_name(0x41).as_deref(), Some("a")); // A
        assert_eq!(vk_key_name(0x5A).as_deref(), Some("z")); // Z
        assert_eq!(vk_key_name(0x30).as_deref(), Some("0"));
        assert_eq!(vk_key_name(0x39).as_deref(), Some("9"));
        assert_eq!(vk_key_name(0x70).as_deref(), Some("f1"));
        assert_eq!(vk_key_name(0x87).as_deref(), Some("f24"));
        // 修饰键（Ctrl 0x11 / Alt 0x12 / Shift 0x10）不作主键
        assert!(vk_key_name(0x11).is_none());
        assert!(vk_key_name(0x12).is_none());
    }
}
