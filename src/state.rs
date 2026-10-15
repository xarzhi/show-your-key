//! 全局应用状态：窗口设置与按键状态，以及设置持久化。

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 设置面板的当前页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    /// 常规设置。
    General,
    /// 外观设置。
    Appearance,
    /// 键位设置。
    Keys,
}

/// 应用设置。
#[derive(Clone, Debug)]
pub struct Settings {
    /// 鼠标穿透。
    pub mouse_passthrough: bool,
    /// 背景透明（不绘制底板，仅显示键帽）。默认开启。
    pub transparent: bool,
    /// 键位透明背景：每个键帽不绘制背景填充，只保留边框与文字。默认关闭。
    pub keycap_transparent: bool,
    /// 简洁模式：隐藏数字键盘、编辑区、方向键、顶部键，只保留主键盘区。
    pub simple_mode: bool,
    /// 暗色主题。
    pub dark: bool,
    /// 开机自启（写入 HKCU Run 键）。
    pub autostart: bool,
    /// 被用户隐藏、不显示的键位（虚拟键码集合）。
    pub hidden_keys: std::collections::HashSet<u16>,
    /// 键盘窗口高度（逻辑像素）。鼠标在键盘上滚轮可调，作为键帽缩放的唯一基准。
    pub keyboard_height: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_passthrough: false,
            // 默认背景透明（用户要求"背景透明改成默认效果"）。
            transparent: true,
            // 键位透明背景默认关闭：默认还是"键帽带底、看得清"的常规外观。
            keycap_transparent: false,
            simple_mode: false,
            dark: false,
            autostart: false,
            hidden_keys: std::collections::HashSet::new(),
            keyboard_height: DEFAULT_KEYBOARD_HEIGHT,
        }
    }
}

/// `serde` 默认值：老版本配置文件里没有 `keyboard_height` 字段时回落到默认高度。
fn default_keyboard_height() -> f32 {
    DEFAULT_KEYBOARD_HEIGHT
}

/// 可持久化的配置（对应 `Settings` 中需要跨进程保存的部分）。
#[derive(Serialize, Deserialize, Debug, Clone)]
struct PersistConfig {
    transparent: bool,
    /// 键位透明背景。
    ///
    /// 必须加 `serde(default)`：这是后加的字段，旧配置文件里没有它，
    /// 缺省会让**整个** `PersistConfig` 反序列化失败，从而把用户所有设置
    /// 一起重置成默认值（而不是只丢这一个字段）。
    #[serde(default)]
    keycap_transparent: bool,
    simple_mode: bool,
    dark: bool,
    autostart: bool,
    hidden_keys: Vec<u16>,
    /// 键盘窗口高度。加 `serde(default)` 保证旧配置（无此字段）仍能正常反序列化。
    #[serde(default = "default_keyboard_height")]
    keyboard_height: f32,
}

impl From<&Settings> for PersistConfig {
    fn from(s: &Settings) -> Self {
        let mut hidden_keys: Vec<u16> = s.hidden_keys.iter().copied().collect();
        hidden_keys.sort_unstable();
        Self {
            transparent: s.transparent,
            keycap_transparent: s.keycap_transparent,
            simple_mode: s.simple_mode,
            dark: s.dark,
            autostart: s.autostart,
            hidden_keys,
            keyboard_height: s.keyboard_height,
        }
    }
}

/// 配置文件的存储路径（`%APPDATA%\show-your-key\config.json`）。
fn config_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("show-your-key").join("config.json")
}

/// 从磁盘加载持久化配置，叠加到 `Settings` 上（加载失败时保持默认值）。
pub fn load_persisted() -> Settings {
    let mut s = Settings::default();
    if let Ok(text) = std::fs::read_to_string(config_path()) {
        if let Ok(cfg) = serde_json::from_str::<PersistConfig>(&text) {
            s.transparent = cfg.transparent;
            s.keycap_transparent = cfg.keycap_transparent;
            s.simple_mode = cfg.simple_mode;
            s.dark = cfg.dark;
            s.autostart = cfg.autostart;
            s.hidden_keys = cfg.hidden_keys.into_iter().collect();
            if cfg.keyboard_height.is_finite() && cfg.keyboard_height > 0.0 {
                s.keyboard_height = cfg.keyboard_height;
            }
        }
    }
    s
}

/// 把 `Settings` 中可持久化的部分写入磁盘（失败静默忽略，不中断主流程）。
pub fn save_persisted(s: &Settings) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let cfg = PersistConfig::from(s);
    if let Ok(text) = serde_json::to_string_pretty(&cfg) {
        let _ = std::fs::write(path, text);
    }
}

/// 拖动状态：记录拖动开始时的全局鼠标位置与窗口位置。
///
/// 坐标统一使用 Windows 虚拟屏幕**物理像素**（由 `win32::cursor_pos` /
/// `win32::window_origin` 取得），这样跨显示器、跨不同 DPI 缩放都能正确还原，
/// 不再依赖 gpui 的逻辑坐标与 `scale_factor`（后者在多屏混合 DPI 下会算错）。
#[derive(Clone, Copy, Debug)]
pub struct DragState {
    /// 拖动开始时的全局鼠标位置（物理像素）。
    pub start_cursor: (i32, i32),
    /// 拖动开始时的窗口左上角位置（物理像素）。
    pub start_origin: (i32, i32),
}/// 全局状态：设置 + 当前按下的按键集合 + 拖动状态。
pub struct AppState {
    pub settings: Settings,
    /// 当前处于按下状态的虚拟键码集合。
    pub pressed: std::collections::HashSet<u32>,
    /// 当前是否正在拖动窗口。
    pub dragging: bool,
    /// 拖动起始信息。
    pub drag: Option<DragState>,
    /// 设置面板当前显示的页面。
    pub settings_page: SettingsPage,
    /// 主键盘窗口当前的高度（逻辑像素）。
    ///
    /// 设置面板里的键盘预览用它反解键帽单位，从而与屏幕上真实键盘保持完全
    /// 一致的缩放（用户调整过键盘窗口大小后，预览也跟着变）。
    pub main_keyboard_height: f32,
    /// 设置面板「键位」页里，按住 Ctrl 划过的**当前**键位。
    ///
    /// 键帽的 `on_hover` 只在悬停状态变化时回调（进入一次、离开一次），这里记下
    /// 当前停留的键位，保证"同一次悬停只切换一次"；离开该键时清空，再次划回来
    /// 还能继续切换。用键位本身而不是布尔量做标记，是为了让"同一帧内先离开 A、
    /// 再进入 B"的事件顺序不影响结果。
    pub ctrl_sweep_key: Option<u16>,
    /// 设置面板是否处于打开状态。
    ///
    /// 主键盘窗口在设置面板打开期间**暂停自适应宽度**：gpui 内部的
    /// `PlatformWindow::resize` 用的是不带 `SWP_NOACTIVATE` 的 `SetWindowPos`，
    /// 会把键盘窗口重新提到最前并抢走焦点，从而压住/顶掉设置面板。
    /// 关闭设置面板后再统一同步一次宽度。
    pub settings_open: bool,
}

/// 主键盘窗口的默认高度（逻辑像素）：2*PADDING + 6 行 * unit(36)。
pub const DEFAULT_KEYBOARD_HEIGHT: f32 = 240.0;

impl AppState {
    pub fn new() -> Self {
        Self {
            settings: load_persisted(),
            pressed: std::collections::HashSet::new(),
            dragging: false,
            drag: None,
            settings_page: SettingsPage::General,
            main_keyboard_height: DEFAULT_KEYBOARD_HEIGHT,
            ctrl_sweep_key: None,
            settings_open: false,
        }
    }
}

impl Global for AppState {}

/// 获取全局状态的可变引用。
pub fn state_mut(cx: &mut App) -> &mut AppState {
    cx.global_mut::<AppState>()
}

/// 获取全局状态的只读引用。
pub fn state(cx: &App) -> &AppState {
    cx.global::<AppState>()
}
