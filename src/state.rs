//! 全局应用状态：窗口设置与按键状态。

use gpui::{App, Global};

/// 应用设置。
#[derive(Clone, Debug)]
pub struct Settings {
    /// 鼠标穿透。
    pub mouse_passthrough: bool,
    /// 背景透明（不绘制底板，仅显示键帽）。
    pub transparent: bool,
    /// 简洁模式：隐藏数字键盘、编辑区、方向键、顶部键，只保留主键盘区。
    pub simple_mode: bool,
    /// 暗色主题。
    pub dark: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_passthrough: false,
            // 默认有背景（不透明），用户右键菜单选择"背景透明"后才变透明。
            transparent: false,
            simple_mode: false,
            dark: false,
        }
    }
}

/// 拖动状态：记录拖动开始时的全局鼠标位置与窗口位置。
#[derive(Clone, Copy, Debug)]
pub struct DragState {
    /// 拖动开始时的全局鼠标位置（屏幕坐标，逻辑像素）。
    pub start_mouse: gpui::Point<gpui::Pixels>,
    /// 拖动开始时的窗口原点（屏幕坐标，逻辑像素）。
    pub start_origin: gpui::Point<gpui::Pixels>,
}/// 全局状态：设置 + 当前按下的按键集合 + 拖动状态。
pub struct AppState {
    pub settings: Settings,
    /// 当前处于按下状态的虚拟键码集合。
    pub pressed: std::collections::HashSet<u32>,
    /// 当前是否正在拖动窗口。
    pub dragging: bool,
    /// 拖动起始信息。
    pub drag: Option<DragState>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            settings: Settings::default(),
            pressed: std::collections::HashSet::new(),
            dragging: false,
            drag: None,
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
