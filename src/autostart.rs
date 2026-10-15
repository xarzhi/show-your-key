//! 开机自启：读写 HKCU\Software\Microsoft\Windows\CurrentVersion\Run 注册表项。
//!
//! 值名固定为 "show-your-key"，值内容为当前 exe 的完整路径（带引号）。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const RUN_KEY: PCWSTR = windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = windows::core::w!("show-your-key");

/// 当前 exe 的完整路径（UTF-16，带引号），用于写入 Run 键。
fn exe_path_utf16() -> Vec<u16> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("\"{}\"", exe).encode_utf16().collect()
}

fn open_or_create_run_key() -> Option<HKEY> {
    unsafe {
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if status == ERROR_SUCCESS {
            Some(key)
        } else {
            None
        }
    }
}

/// 开启或关闭开机自启。返回是否成功。
pub fn set_autostart(enabled: bool) -> bool {
    unsafe {
        let Some(key) = open_or_create_run_key() else {
            return false;
        };
        let result = if enabled {
            let data = exe_path_utf16();
            // 转成字节 slice（每 u16 两字节，不含结尾 null）。
            let bytes: Vec<u8> = data.iter().flat_map(|u| u.to_le_bytes()).collect();
            RegSetValueExW(key, VALUE_NAME, None, REG_SZ, Some(&bytes)) == ERROR_SUCCESS
        } else {
            let status = RegDeleteValueW(key, VALUE_NAME);
            status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
        };
        let _ = RegCloseKey(key);
        result
    }
}
