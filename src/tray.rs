//! 系统托盘图标。
//!
//! 在独立线程创建隐藏消息窗口并注册 Shell_NotifyIcon 托盘图标：
//! - 左键单击托盘图标：显示/隐藏主窗口
//! - 右键单击托盘图标：弹出菜单（显示/隐藏、鼠标穿透、退出）
//!
//! "退出"会删除托盘图标并终止进程（gpui 在所有窗口关闭后不会自动退出）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetMessageW, GetCursorPos, GetWindowLongW, IsWindowVisible, LoadImageW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, TrackPopupMenu,
    DestroyWindow, GWL_EXSTYLE, HMENU, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, MF_STRING,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOW, TPM_BOTTOMALIGN,
    TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WINDOW_EX_STYLE,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

/// 托盘回调消息（WM_APP 基址）。
const TRAY_CALLBACK_MSG: u32 = WM_APP + 1;
/// 菜单项 ID。
const MENU_TOGGLE_VISIBLE: usize = 1;
const MENU_TOGGLE_PASSTHROUGH: usize = 2;
const MENU_QUIT: usize = 3;

/// 主窗口句柄（由主线程在窗口创建后设置）。
static MAIN_HWND: OnceLock<isize> = OnceLock::new();
/// 与 UI 全局状态同步的穿透状态（供托盘菜单显示正确的菜单文字）。
pub static PASSTHROUGH: AtomicBool = AtomicBool::new(false);

/// UTF-8 → null 结尾的 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动托盘线程。`main_hwnd` 为主窗口句柄。
pub fn start(main_hwnd: HWND) {
    let _ = MAIN_HWND.set(main_hwnd.0 as isize);
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(run_tray_thread)
        .expect("failed to spawn tray thread");
}

/// 当前是否鼠标穿透。
pub fn is_passthrough() -> bool {
    PASSTHROUGH.load(Ordering::Relaxed)
}

/// 更新穿透状态（由 UI 侧菜单调用，保持托盘菜单同步）。
pub fn set_passthrough(v: bool) {
    PASSTHROUGH.store(v, Ordering::Relaxed);
}

fn main_hwnd() -> HWND {
    HWND(MAIN_HWND.get().copied().unwrap_or(0) as *mut _)
}

fn run_tray_thread() {
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null()).expect("GetModuleHandleW failed");
        let hinstance = HINSTANCE(hinstance.0);
        let class_name = wide("sk_tray_host");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // 隐藏的消息窗口：接收托盘回调。
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR::from_raw(class_name.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
        .expect("failed to create tray host window");

        // 加载嵌入的图标资源（资源 ID 1，由 build.rs 嵌入）。
        // 注意：MAKEINTRESOURCE(1) 必须是精确的地址值 1，不能用
        // std::ptr::dangling（那会返回对齐值 2，导致资源 ID 错误）。
        let icon = LoadImageW(
            Some(hinstance),
            #[allow(clippy::manual_dangling_ptr)]
            PCWSTR::from_raw(1 as *const u16), // MAKEINTRESOURCE(1)
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
        .expect("failed to load app icon resource");

        // 注册托盘图标。
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MSG,
            hIcon: windows::Win32::UI::WindowsAndMessaging::HICON(icon.0),
            ..Default::default()
        };
        for (i, c) in "show-your-key".encode_utf16().take(127).enumerate() {
            nid.szTip[i] = c;
        }
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        // 消息循环。
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = DispatchMessageW(&msg);
        }

        // 退出前删除托盘图标。
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = DestroyWindow(hwnd);
    }
}

extern "system" fn tray_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_COMMAND => {
                match wparam.0 & 0xffff {
                    MENU_TOGGLE_VISIBLE => toggle_main_window(),
                    MENU_TOGGLE_PASSTHROUGH => toggle_passthrough(),
                    MENU_QUIT => quit(),
                    _ => {}
                }
                LRESULT(0)
            }
            m if m == TRAY_CALLBACK_MSG => {
                match lparam.0 as u32 {
                    // 左键：切换主窗口显示/隐藏。
                    WM_LBUTTONUP => toggle_main_window(),
                    // 右键：弹出菜单。
                    WM_RBUTTONUP => show_context_menu(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// 切换主窗口显示/隐藏。
fn toggle_main_window() {
    unsafe {
        let hwnd = main_hwnd();
        if hwnd.is_invalid() {
            return;
        }
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// 切换鼠标穿透（直接改主窗口 Win32 样式，并同步状态标记）。
fn toggle_passthrough() {
    unsafe {
        let hwnd = main_hwnd();
        if hwnd.is_invalid() {
            return;
        }
        let new_state = !is_passthrough();
        set_passthrough(new_state);
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let mut style = WINDOW_EX_STYLE(ex_style as u32);
        if new_state {
            style |= WS_EX_LAYERED | WS_EX_TRANSPARENT;
        } else {
            style &= WINDOW_EX_STYLE(!(WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0));
        }
        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, style.0 as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

/// 弹出托盘右键菜单。
fn show_context_menu(hwnd: HWND) {
    unsafe {
        let menu: HMENU = CreatePopupMenu().expect("CreatePopupMenu failed");
        let visible = IsWindowVisible(main_hwnd()).as_bool();
        let show_label = wide(if visible { "隐藏键盘" } else { "显示键盘" });
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            MENU_TOGGLE_VISIBLE,
            PCWSTR::from_raw(show_label.as_ptr()),
        );
        let pt_label = wide(if is_passthrough() {
            "取消鼠标穿透"
        } else {
            "鼠标穿透"
        });
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            MENU_TOGGLE_PASSTHROUGH,
            PCWSTR::from_raw(pt_label.as_ptr()),
        );
        let quit_label = wide("退出");
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            MENU_QUIT,
            PCWSTR::from_raw(quit_label.as_ptr()),
        );

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

/// 退出程序：删除托盘图标并终止进程。
fn quit() {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: main_hwnd(),
            uID: 1,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        std::process::exit(0);
    }
}
