//! 构建脚本：将 app.ico 嵌入 exe 资源（程序图标 + 供托盘/窗口图标加载）。
//!
//! winresource 默认通过 reg.exe 查询 Windows SDK 位置，在某些受限环境下
//! 会被拦截。这里自行在常见安装位置查找 rc.exe，并通过 RC_PATH 环境变量
//! 提供给 winresource，绕过注册表查询。

use std::path::{Path, PathBuf};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // 1. 用户显式指定优先。
    if std::env::var_os("RC_PATH").is_none() {
        if let Some(rc) = find_rc_exe() {
            std::env::set_var("RC_PATH", &rc);
        }
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    winresource::WindowsResource::new()
        .set_icon(&format!("{}\\app.ico", manifest_dir))
        .compile()
        .expect("failed to embed app.ico into executable resources");
}

/// 在常见 Windows Kits 安装位置查找 rc.exe（取版本号最新的一个）。
fn find_rc_exe() -> Option<PathBuf> {
    // 已显式设置则直接使用。
    if let Some(p) = std::env::var_os("RC_PATH") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["WindowsSdkDir", "WindowsSdkBinPath", "WindowsSdkVerBinPath"] {
        if let Some(v) = std::env::var_os(var) {
            roots.push(PathBuf::from(v));
        }
    }
    for drive in ["C:", "D:", "E:"] {
        roots.push(PathBuf::from(format!(
            "{}\\Program Files (x86)\\Windows Kits\\10",
            drive
        )));
        roots.push(PathBuf::from(format!(
            "{}\\Program Files\\Windows Kits\\10",
            drive
        )));
        // 自定义盘根直接装 Windows Kits 的情况（如 D:\Windows Kits\10）。
        roots.push(PathBuf::from(format!("{}\\Windows Kits\\10", drive)));
    }

    for root in roots {
        let bin = root.join("bin");
        if !bin.is_dir() {
            continue;
        }
        // 枚举 bin 下的版本目录（如 10.0.22621.0），取最新。
        let mut versions: Vec<(String, PathBuf)> = std::fs::read_dir(&bin)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
            .collect();
        // 版本目录名形如 10.0.22621.0，按数值降序排。
        versions.sort_by_key(|a| std::cmp::Reverse(version_key(&a.0)));
        for (_, dir) in versions {
            // 标准布局：<root>\bin\<ver>\x64\rc.exe；老布局 <root>\bin\x64\rc.exe。
            for candidate in [
                dir.join("x64").join("rc.exe"),
                dir.join("bin").join("x64").join("rc.exe"),
            ] {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// "10.0.22621.0" → [10, 0, 22621, 0]，用于排序。
fn version_key(name: &str) -> Vec<u32> {
    name.split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}

#[allow(dead_code)]
fn _unused(p: &Path) -> bool {
    p.exists()
}
