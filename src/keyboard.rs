//! 键盘布局定义：按真实键盘的物理排列组织键位。
//!
//! 坐标系：以键盘主区左上角为原点，横向单位为 1u（标准键宽），纵向为 1u。
//! 每行按键通过 `offset` 控制该行起始的横向缩进，`w` 表示键宽（u）。

/// 一个物理按键的静态描述。
pub struct KeyDef {
    /// 键帽上显示的主标签（如 "A"、"Enter"、"⇧"）。
    pub label: &'static str,
    /// 该键对应的扫描码（Windows 虚拟键码 VK_* 的值），用于匹配按下事件。
    pub vk: u16,
    /// 行索引（从 0 开始，功能键行视为第 0 行）。
    pub row: usize,
    /// 该键在本行内的横向起始位置（单位 u，从行起点算起）。
    pub x: f32,
    /// 键宽（单位 u，标准键为 1.0）。
    pub w: f32,
    /// 键高（单位 u，标准键为 1.0）。
    pub h: f32,
    /// 是否为数字小键盘区的键（用于"显示数字键盘"开关）。
    pub numpad: bool,
    /// 是否为导航/编辑区键（Insert/Home/PgUp/方向键/PrtSc 等，用于"简洁模式"）。
    pub nav: bool,
}

/// 全键盘布局。行从功能键行开始依次向下。
pub struct Layout {
    pub keys: Vec<KeyDef>,
}

// Windows 虚拟键码常量（部分）。
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_TAB: u16 = 0x09;
pub const VK_CAPITAL: u16 = 0x14;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_BACK: u16 = 0x08;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SPACE: u16 = 0x20;
pub const VK_PRIOR: u16 = 0x21; // PageUp
pub const VK_NEXT: u16 = 0x22; // PageDown
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_SNAPSHOT: u16 = 0x2C; // PrintScreen
pub const VK_INSERT: u16 = 0x2D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_SCROLL: u16 = 0x91; // ScrollLock
pub const VK_PAUSE: u16 = 0x13; // Pause/Break

/// 构建标准 ANSI 104 键布局。
pub fn standard_layout() -> Layout {
    // 功能键行（Esc + F1-F12）：Esc 用真实 vk，F 键用 0x70..0x7B。
    let mut keys: Vec<KeyDef> = Vec::new();

    // 行 0：Esc  F1 F2 F3 F4  F5 F6 F7 F8  F9 F10 F11 F12
    keys.push(key("Esc", VK_ESCAPE, 0, 0.0, 1.0, false));
    for i in 0..12u16 {
        let x = 2.0 + i as f32; // F1 从 x=2 开始，每个间隔 1u，F4 与 F5 之间留 0.5 间隔
        // F4 后加 0.5 间隔
        let x = if i >= 4 { x + 0.5 } else { x };
        let x = if i >= 8 { x + 0.5 } else { x };
        keys.push(key("", 0x70 + i, 0, x, 1.0, false));
    }
    // 用真实标签覆盖 F 键
    for (i, lbl) in ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"]
        .iter()
        .enumerate()
    {
        keys[1 + i].label = lbl;
    }
    // 行 0 顶部 3 键：PrintScreen / ScrollLock / Pause（编辑区上方，x=15.45/16.45/17.45）。
    keys.push(key_nav("PrtSc", VK_SNAPSHOT, 0, 15.45));
    keys.push(key_nav("ScrLk", VK_SCROLL, 0, 16.45));
    keys.push(key_nav("Pause", VK_PAUSE, 0, 17.45));

    // 行 1：` 1 2 3 4 5 6 7 8 9 0 - = Backspace
    let row1: [(&str, u16, f32, f32); 14] = [
        ("`", 0xC0, 0.0, 1.0),
        ("1", 0x31, 1.0, 1.0),
        ("2", 0x32, 2.0, 1.0),
        ("3", 0x33, 3.0, 1.0),
        ("4", 0x34, 4.0, 1.0),
        ("5", 0x35, 5.0, 1.0),
        ("6", 0x36, 6.0, 1.0),
        ("7", 0x37, 7.0, 1.0),
        ("8", 0x38, 8.0, 1.0),
        ("9", 0x39, 9.0, 1.0),
        ("0", 0x30, 10.0, 1.0),
        ("-", 0xBD, 11.0, 1.0),
        ("=", 0xBB, 12.0, 1.0),
        ("⌫", VK_BACK, 13.0, 2.0),
    ];
    for (l, vk, x, w) in row1 {
        keys.push(key(l, vk, 1, x, w, false));
    }

    // 行 2：Tab Q W E R T Y U I O P [ ] \
    let row2: [(&str, u16, f32, f32); 14] = [
        ("Tab", VK_TAB, 0.0, 1.5),
        ("Q", 0x51, 1.5, 1.0),
        ("W", 0x57, 2.5, 1.0),
        ("E", 0x45, 3.5, 1.0),
        ("R", 0x52, 4.5, 1.0),
        ("T", 0x54, 5.5, 1.0),
        ("Y", 0x59, 6.5, 1.0),
        ("U", 0x55, 7.5, 1.0),
        ("I", 0x49, 8.5, 1.0),
        ("O", 0x4F, 9.5, 1.0),
        ("P", 0x50, 10.5, 1.0),
        ("[", 0xDB, 11.5, 1.0),
        ("]", 0xDD, 12.5, 1.0),
        ("\\", 0xDC, 13.5, 1.5),
    ];
    for (l, vk, x, w) in row2 {
        keys.push(key(l, vk, 2, x, w, false));
    }

    // 行 3：Caps A S D F G H J K L ; ' Enter
    let row3: [(&str, u16, f32, f32); 13] = [
        ("Caps", VK_CAPITAL, 0.0, 1.75),
        ("A", 0x41, 1.75, 1.0),
        ("S", 0x53, 2.75, 1.0),
        ("D", 0x44, 3.75, 1.0),
        ("F", 0x46, 4.75, 1.0),
        ("G", 0x47, 5.75, 1.0),
        ("H", 0x48, 6.75, 1.0),
        ("J", 0x4A, 7.75, 1.0),
        ("K", 0x4B, 8.75, 1.0),
        ("L", 0x4C, 9.75, 1.0),
        (";", 0xBA, 10.75, 1.0),
        ("'", 0xDE, 11.75, 1.0),
        ("Enter", VK_RETURN, 12.75, 2.25),
    ];
    for (l, vk, x, w) in row3 {
        keys.push(key(l, vk, 3, x, w, false));
    }

    // 行 4：Shift Z X C V B N M , . / Shift
    let row4: [(&str, u16, f32, f32); 12] = [
        ("Shift", VK_LSHIFT, 0.0, 2.25),
        ("Z", 0x5A, 2.25, 1.0),
        ("X", 0x58, 3.25, 1.0),
        ("C", 0x43, 4.25, 1.0),
        ("V", 0x56, 5.25, 1.0),
        ("B", 0x42, 6.25, 1.0),
        ("N", 0x4E, 7.25, 1.0),
        ("M", 0x4D, 8.25, 1.0),
        (",", 0xBC, 9.25, 1.0),
        (".", 0xBE, 10.25, 1.0),
        ("/", 0xBF, 11.25, 1.0),
        ("Shift", VK_RSHIFT, 12.25, 2.75),
    ];
    for (l, vk, x, w) in row4 {
        keys.push(key(l, vk, 4, x, w, false));
    }

    // 行 5：Ctrl Win Alt Space Alt Win Menu Ctrl
    let row5: [(&str, u16, f32, f32); 8] = [
        ("Ctrl", VK_LCONTROL, 0.0, 1.25),
        ("Win", VK_LWIN, 1.25, 1.25),
        ("Alt", VK_LMENU, 2.5, 1.25),
        ("", VK_SPACE, 3.75, 6.25),
        ("Alt", VK_RMENU, 10.0, 1.25),
        ("Win", VK_RWIN, 11.25, 1.25),
        ("Menu", 0x5D, 12.5, 1.25),
        ("Ctrl", VK_RCONTROL, 13.75, 1.25),
    ];
    for (l, vk, x, w) in row5 {
        keys.push(key(l, vk, 5, x, w, false));
    }

    // 编辑控制区（主键盘与数字键盘之间，3 列，x=15.45/16.45/17.45，左右各约 0.45u 间隙，对称）：
    // Insert/Home/PageUp（上），Delete/End/PageDown（下）。
    let edit: [(&str, u16, usize, f32); 6] = [
        ("Insert", VK_INSERT, 1, 15.45),
        ("Home", VK_HOME, 1, 16.45),
        ("PgUp", VK_PRIOR, 1, 17.45),
        ("Delete", VK_DELETE, 2, 15.45),
        ("End", VK_END, 2, 16.45),
        ("PgDn", VK_NEXT, 2, 17.45),
    ];
    for (l, vk, r, x) in edit {
        keys.push(key_nav(l, vk, r, x));
    }

    // 方向键（第 4、5 行，位于编辑区下方，x=15.45/16.45/17.45）。
    keys.push(key_nav("↑", VK_UP, 4, 16.45));
    keys.push(key_nav("←", VK_LEFT, 5, 15.45));
    keys.push(key_nav("↓", VK_DOWN, 5, 16.45));
    keys.push(key_nav("→", VK_RIGHT, 5, 17.45));

    // 数字小键盘区（行 0-4 的右侧，起始 x 18.5）。
    // 行 0：Num Lock / * -
    // 行 1：7 8 9 +
    // 行 2：4 5 6
    // 行 3：1 2 3 Enter
    // 行 4：0 . Enter
    // 数字小键盘区（行 0-4 的右侧，起始 x 18.9，与编辑区之间约 0.45u 间隙，左右对称）。
    let np: [(&str, u16, f32, f32, f32); 17] = [
        ("Num", 0x90, 18.9, 0.0, 1.0),
        ("/", 0x6F, 19.9, 0.0, 1.0),
        ("*", 0x6A, 20.9, 0.0, 1.0),
        ("-", 0x6D, 21.9, 0.0, 1.0),
        ("7", 0x67, 18.9, 1.0, 1.0),
        ("8", 0x68, 19.9, 1.0, 1.0),
        ("9", 0x69, 20.9, 1.0, 1.0),
        ("+", 0x6B, 21.9, 1.0, 1.0),
        ("4", 0x64, 18.9, 2.0, 1.0),
        ("5", 0x65, 19.9, 2.0, 1.0),
        ("6", 0x66, 20.9, 2.0, 1.0),
        ("1", 0x61, 18.9, 3.0, 1.0),
        ("2", 0x62, 19.9, 3.0, 1.0),
        ("3", 0x63, 20.9, 3.0, 1.0),
        ("Enter", 0x6C, 21.9, 3.0, 1.0),
        ("0", 0x60, 18.9, 4.0, 2.0),
        (".", 0x6E, 20.9, 4.0, 1.0),
    ];
    for (l, vk, x, r, w) in np {
        let mut k = key(l, vk, r as usize, x, w, true);
        // "+" 键高 2u（跨第 1、2 行）
        if vk == 0x6B {
            k.h = 2.0;
        }
        // 小键盘 Enter 键高 2u（跨第 3、4 行）
        if vk == 0x6C {
            k.h = 2.0;
        }
        keys.push(k);
    }

    Layout { keys }
}

fn key(label: &'static str, vk: u16, row: usize, x: f32, w: f32, numpad: bool) -> KeyDef {
    KeyDef {
        label,
        vk,
        row,
        x,
        w,
        h: 1.0,
        numpad,
        nav: false,
    }
}

/// 导航/编辑区键（Insert/Home/PgUp/方向键/PrtSc 等），宽 1u，标记 nav=true。
fn key_nav(label: &'static str, vk: u16, row: usize, x: f32) -> KeyDef {
    KeyDef {
        label,
        vk,
        row,
        x,
        w: 1.0,
        h: 1.0,
        numpad: false,
        nav: true,
    }
}
