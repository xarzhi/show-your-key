//! 全局键盘监听：使用低级键盘钩子 WH_KEYBOARD_LL 在后台线程捕获全局按键。
//!
//! 由于窗口在录屏时通常不处于焦点状态，gpui 自身无法收到键盘事件，
//! 因此需要一个独立的 Win32 低级键盘钩子来监听全局按键，并把按下/抬起
//! 事件通过 channel 发送给 UI 线程。

use std::sync::mpsc::{Receiver, Sender};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, MSG,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, LLKHF_EXTENDED,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// 一个全局按键事件。
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    /// 虚拟键码（VK_*）。小键盘 Enter 被映射为 `VK_SEPARATOR`(0x6C) 以区别于主键盘 Enter。
    pub vk: u32,
    /// 是否按下（true=down，false=up）。
    pub pressed: bool,
}

/// 小键盘 Enter 键的标识码（用 VK_SEPARATOR 占位，避免与主键盘 Enter 的 0x0D 冲突）。
pub const NUMPAD_ENTER: u32 = 0x6C;

/// 启动全局键盘钩子，返回接收事件的 channel。
///
/// 该函数会启动一个后台线程运行消息循环，钩子安装在该线程上。
/// 返回的 Receiver 用于读取按键事件。
pub fn start() -> Receiver<KeyEvent> {
    let (tx, rx) = std::sync::mpsc::channel::<KeyEvent>();
    std::thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(move || {
            run_hook_thread(tx);
        })
        .expect("failed to spawn keyboard hook thread");
    rx
}

fn run_hook_thread(tx: Sender<KeyEvent>) {
    let hinstance = unsafe { GetModuleHandleW(None).ok() }.map(|h| HINSTANCE(h.0));

    let hook = unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), hinstance, 0)
    };

    // 无论钩子是否安装成功，都进入消息循环，保证线程存活并能退出。
    let _hook = HookGuard(hook.ok());

    // 把 Sender 存到线程局部，供回调使用。
    let tx: &'static Sender<KeyEvent> = Box::leak(Box::new(tx));
    THREAD_SENDER.with(|slot| {
        *slot.borrow_mut() = Some(tx);
    });

    let mut msg = MSG::default();
    loop {
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct HookGuard(Option<HHOOK>);

impl Drop for HookGuard {
    fn drop(&mut self) {
        if let Some(hook) = self.0 {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    }
}

thread_local! {
    static THREAD_SENDER: std::cell::RefCell<Option<&'static Sender<KeyEvent>>> =
        const { std::cell::RefCell::new(None) };
}

extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let info = unsafe { *(l_param.0 as *const KBDLLHOOKSTRUCT) };
        let msg = w_param.0 as u32;
        let pressed = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
        let is_key = matches!(msg, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP);
        if is_key {
            // 小键盘 Enter：scanCode 为 0x1C 且带 LLKHF_EXTENDED 标志，
            // 与主键盘 Enter（同 0x0D）区分，映射为 NUMPAD_ENTER。
            let extended = info.flags.contains(LLKHF_EXTENDED);
            let vk = if info.vkCode == 0x0D && extended {
                NUMPAD_ENTER
            } else {
                info.vkCode
            };
            THREAD_SENDER.with(|slot| {
                if let Some(tx) = *slot.borrow() {
                    let _ = tx.send(KeyEvent { vk, pressed });
                }
				
            });
        }
    }
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}
