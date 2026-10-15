//! Win32 窗口控制：置顶、鼠标穿透、去边框阴影、窗口图标、移动、隐藏任务栏。
//!
//! 通过 raw-window-handle 获取窗口的 HWND，再调用 Win32 API 实现。
//! 窗口背景透明由 gpui 的 `WindowBackgroundAppearance::Transparent`（Direct Composition）处理，
//! 实际背景色/透明完全由 view 层的 `.bg()` 控制。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMNCRENDERINGPOLICY, DWMWA_NCRENDERING_POLICY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, LoadImageW, SendMessageW, SetWindowLongW, SetWindowPos, GWL_EXSTYLE,
    HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINDOW_EX_STYLE,
    WM_SETICON, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// 从任意实现 `HasWindowHandle` 的窗口对象中提取 HWND。
pub fn hwnd_from(window: &impl HasWindowHandle) -> Option<HWND> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// 初始化窗口外观：禁用 DWM 非客户区渲染（去掉边框和阴影）。
///
/// 保留 WS_THICKFRAME 以维持窗口可缩放的热区，但 DWM 不再绘制边框和阴影。
pub fn remove_border_and_shadow(hwnd: HWND) {
    unsafe {
        // DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED(1)：不绘制非客户区（边框+阴影）。
        let policy = DWMNCRENDERINGPOLICY(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &policy as *const _ as *const _,
            std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
        );
        // 触发非客户区重算，让设置立即生效。
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

/// 设置窗口是否置顶。
pub fn set_always_on_top(hwnd: HWND, on_top: bool) {
    unsafe {
        let insert_after = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        let _ = SetWindowPos(
            hwnd,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// 让窗口不在任务栏显示（只保留托盘图标）。
///
/// 通过给窗口加 `WS_EX_TOOLWINDOW` 扩展样式：工具窗口不会出现在任务栏，
/// 也不会出现在 Alt+Tab 切换列表。同时移除 `WS_EX_APPWINDOW`（该样式会强制
/// 窗口显示在任务栏）。
pub fn hide_from_taskbar(hwnd: HWND) {
    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let mut style = WINDOW_EX_STYLE(ex_style as u32);
        // 移除 APPWINDOW（强制出现在任务栏的标志），加上 TOOLWINDOW（工具窗口）。
        style &= WINDOW_EX_STYLE(!WS_EX_APPWINDOW.0);
        style |= WS_EX_TOOLWINDOW;
        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, style.0 as i32);
        // 触发样式重算。
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

/// 设置窗口是否鼠标穿透（点击穿透到下层窗口）。
///
/// 注意：WS_EX_TRANSPARENT 必须与 WS_EX_LAYERED 组合才能实现点击穿透。
pub fn set_mouse_passthrough(hwnd: HWND, passthrough: bool) {
    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let mut style = WINDOW_EX_STYLE(ex_style as u32);
        if passthrough {
            style |= WS_EX_TRANSPARENT | WS_EX_LAYERED;
        } else {
            style &= WINDOW_EX_STYLE(!(WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0));
        }
        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, style.0 as i32);
    }
}

/// 从 exe 资源加载图标并设置为窗口图标（任务栏 + alt-tab）。
///
/// 资源 ID 1 由 build.rs 通过 winresource 嵌入的 app.ico 提供。
pub fn set_window_icon(hwnd: HWND) {
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null()).ok();
        let hinstance = hinstance.map(|h| HINSTANCE(h.0));
        if let Ok(handle) = LoadImageW(
            hinstance,
            // MAKEINTRESOURCE(1) 必须是精确地址值 1，不能用 dangling（返回对齐值 2）。
            #[allow(clippy::manual_dangling_ptr)]
            PCWSTR::from_raw(1 as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        ) {
            let hicon = windows::Win32::UI::WindowsAndMessaging::HICON(handle.0);
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(windows::Win32::Foundation::WPARAM(ICON_BIG as usize)),
                Some(windows::Win32::Foundation::LPARAM(hicon.0 as isize)),
            );
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(windows::Win32::Foundation::WPARAM(ICON_SMALL as usize)),
                Some(windows::Win32::Foundation::LPARAM(hicon.0 as isize)),
            );
        }
    }
}

/// 移动窗口到指定屏幕坐标（物理像素）。
pub fn move_window(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER);
    }
}
