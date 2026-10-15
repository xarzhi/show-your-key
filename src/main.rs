//! show-your-key：录屏用键盘键位显示程序。
#![windows_subsystem = "windows"]
//!
//! 功能：
//! - 始终置顶的无边框窗口，展示完整键盘布局
//! - 全局键盘监听，按键按下时对应键位高亮（脱离窗口焦点，任意窗口输入都生效）
//! - 整个窗口可按住鼠标拖动
//! - 右键菜单：鼠标穿透 / 背景透明 / 显示数字键盘 / 主题切换
//! - 窗口可缩放，且保持固定宽高比
//!
//! 注意：这里直接用底层 `gpui`（gpui-pre）打开窗口，而不经过 `gpui_kit::open_window`。
//! 原因：gpui-kit 会把视图包进 `gpui_base::Root`，而 gpui-component 的 `WindowState`
//! 插件会强制给 root surface 注入主题背景色（亮色即白色），导致"背景透明"永远无效。
//! 直接开窗口后，背景透明完全由 `WindowBackgroundAppearance::Transparent` 控制。

mod autostart;
mod keyboard;
mod keyhook;
mod settings_window;
mod state;
mod tray;
mod win32;

use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui_kit::{
    div, px, rgba, AppContext, Bounds, Context, ParentElement, Render, Size, Styled, Window,
    WindowOptions, InteractiveElement, IntoElement, MouseButton,
};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::theme::{Theme, ThemeMode};

use crate::keyboard::{standard_layout, KeyDef, Layout};
use crate::state::{state, state_mut, AppState};

/// 主键盘区逻辑宽度（单位 u，0..15，简洁模式）。
pub(crate) const SIMPLE_EXTENT_W: f32 = 15.0;
/// 全键盘逻辑宽度（单位 u，0..22.9，含编辑区/方向键/顶部键 + 数字键盘）。
pub(crate) const NUMPAD_EXTENT_W: f32 = 22.9;
/// 键盘逻辑高度（单位 u，6 行）。
pub(crate) const EXTENT_H: f32 = 6.0;

/// 窗口内边距（像素，固定值，不随窗口缩放）。
pub(crate) const PADDING: f32 = 12.0;
/// 键帽之间的间隙（像素，固定值）。
pub(crate) const GAP: f32 = 3.0;

/// 由窗口高度计算键帽单位（u → 像素）。
/// 高度 = 2*PADDING + EXTENT_H*unit，反解 unit。
pub(crate) fn unit_from_height(h: f32) -> f32 {
    ((h - PADDING * 2.0) / EXTENT_H).max(1.0)
}

/// 由键帽单位计算键帽字号（与主窗口 render_keyboard 完全一致）。
pub(crate) fn font_size_for(unit: f32) -> f32 {
    (unit * 0.24).max(7.0)
}

/// 窗口允许的最小尺寸（与 `WindowOptions::window_min_size` 保持一致）。
const MIN_WINDOW_W: f32 = 300.0;
const MIN_WINDOW_H: f32 = 120.0;
/// 键帽单位的缩放上下限（滚轮缩放时钳制）。
const MIN_UNIT: f32 = 8.0;
const MAX_UNIT: f32 = 110.0;
/// 滚轮每档（一格 ≈ 3 行）对应的缩放比例。
const ZOOM_PER_NOTCH: f32 = 0.075;
/// 尺寸比较容差（逻辑像素）。
///
/// 平台窗口尺寸是整数像素，我们请求的 299.0 可能被系统按最小宽度钳成 300.0，
/// 于是"期望 299 / 实际 300"永远差 1px —— 若用 0.5 做阈值，`resize` 会在每次
/// 尺寸变化时被反复调用。留 2px 容差可吸收这类量化误差，同时不影响正常的等比校正
/// （滚轮每档变化十几像素）。
const SIZE_EPS: f32 = 2.0;

/// 由键帽单位和显示模式，计算窗口应有的宽度。
/// 宽度 = 2*PADDING + extent_w*unit。
/// 简洁模式 = 只留主键盘区（15u）；否则 = 全键盘（含数字键盘，22.9u）。
fn width_for(unit: f32, simple: bool) -> f32 {
    let extent_w = if simple { SIMPLE_EXTENT_W } else { NUMPAD_EXTENT_W };
    PADDING * 2.0 + extent_w * unit
}

/// 当前窗口所在显示器的工作区尺寸（逻辑像素，已排除任务栏）。
fn work_area(window: &Window, cx: &gpui_kit::App) -> (f32, f32) {
    match window.display(cx) {
        Some(display) => {
            let b = display.visible_bounds();
            (b.size.width.as_f32(), b.size.height.as_f32())
        }
        None => (f32::INFINITY, f32::INFINITY),
    }
}

/// 由「期望高度 + 显示模式 + 显示器工作区」算出最终合法的窗口尺寸 `(width, height)`。
///
/// 这里做三件事：
/// 1. 把高度钳制在允许范围内（滚轮缩放不会把键盘缩到不可用，也不会超出屏幕）；
/// 2. 保证算出的宽度不低于平台窗口的最小宽度 —— 否则 `SetWindowPos` 会被系统钳制，
///    而 `observe_window_bounds` 看到的尺寸与期望值永远不一致，会反复触发 resize；
/// 3. 不允许宽/高超过当前显示器工作区（键盘是录屏悬浮窗，放大到屏幕外没有意义）。
fn desired_size(height: f32, simple: bool, work_w: f32, work_h: f32) -> (f32, f32) {
    let extent_w = if simple { SIMPLE_EXTENT_W } else { NUMPAD_EXTENT_W };
    // 由最小窗口宽度反解出的高度下限。
    let min_h = (PADDING * 2.0 + EXTENT_H * ((MIN_WINDOW_W - PADDING * 2.0) / extent_w))
        .max(MIN_WINDOW_H)
        .max(PADDING * 2.0 + EXTENT_H * MIN_UNIT);
    // 高度上限：先取键帽单位上限，再按显示器工作区高/宽收窄。
    let mut max_h = PADDING * 2.0 + EXTENT_H * MAX_UNIT;
    if work_h.is_finite() {
        max_h = max_h.min(work_h);
    }
    if work_w.is_finite() {
        max_h = max_h.min(PADDING * 2.0 + EXTENT_H * ((work_w - PADDING * 2.0) / extent_w));
    }
    let max_h = max_h.max(min_h);
    let h = if height.is_finite() {
        height.clamp(min_h, max_h)
    } else {
        min_h
    };
    (width_for(unit_from_height(h), simple), h)
}

/// 根视图。
struct KeyView {
    layout: Layout,
    /// 全局按键事件接收端。
    key_rx: Receiver<keyhook::KeyEvent>,
    /// 托盘事件接收端（如"打开设置面板"）。
    tray_rx: Receiver<tray::TrayEvent>,
    /// 窗口尺寸变化订阅（必须持有，否则观察器被丢弃）。
    _bounds_subscription: gpui_kit::Subscription,
    /// 全局状态订阅（设置变化时刷新主窗口，必须持有）。
    _global_subscription: gpui_kit::Subscription,
}

impl KeyView {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_global(AppState::new());
        let key_rx = keyhook::start();

        // 建立托盘事件通道，主窗口异步循环监听"打开设置面板"等事件。
        let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayEvent>();
        tray::set_event_tx(tray_tx);

        // 监听全局状态变化（设置面板改设置后，主窗口自动刷新）。
        let global_subscription = cx.observe_global::<AppState>(|_view, cx| {
            cx.notify();
        });

        // 监听窗口尺寸变化，保持键盘比例（以高度为基准）。
        let bounds_subscription = cx.observe_window_bounds(_window, |_view, window, cx| {
            let s = &state(cx).settings;
            let open = state(cx).settings_open;
            let bounds = window.bounds();
            let w = bounds.size.width.as_f32();
            let h = bounds.size.height.as_f32();
            // 以【高度】为基准：unit 由高度决定，宽度由 unit 和显示模式决定。
            // 这样主键盘区（左边）大小只随高度变化，右侧区域增减只改变宽度。
            let (wa_w, wa_h) = work_area(window, cx);
            let (expected_w, expected_h) = desired_size(h, s.simple_mode, wa_w, wa_h);
            // 设置面板打开期间不自动改尺寸：gpui 内部 resize 会把键盘窗口提到最前
            // 并抢焦点，压住设置面板。关闭面板后 render() 会再同步一次。
            if !open && ((w - expected_w).abs() > SIZE_EPS || (h - expected_h).abs() > SIZE_EPS) {
                window.resize(Size {
                    width: px(expected_w),
                    height: px(expected_h),
                });
            }
            // 把当前高度记进全局状态：设置面板的键盘预览据此与屏幕上的键盘等比。
            if (state(cx).main_keyboard_height - h).abs() > 0.5 {
                state_mut(cx).main_keyboard_height = h;
            }
            cx.notify();
        });

        // 关键：用独立异步循环持续读取全局按键事件并刷新 UI。
        // 不能依赖 render() 被触发——窗口失焦且无变化时 render() 根本不执行，
        // 导致非聚焦输入不亮、抬起后不灭。
        cx.spawn(async move |this: gpui_kit::WeakEntity<KeyView>, cx: &mut gpui_kit::AsyncApp| {
            loop {
                // 阻塞式接收事件（有事件立即返回，无事件等待），收到后立刻刷新。
                match this.update(cx, |view, cx| {
                    let mut changed = false;
                    while let Ok(ev) = view.key_rx.try_recv() {
                        changed = true;
                        if ev.pressed {
                            state_mut(cx).pressed.insert(ev.vk);
                        } else {
                            state_mut(cx).pressed.remove(&ev.vk);
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                    // 处理托盘事件（如"打开设置面板"）。
                    while let Ok(ev) = view.tray_rx.try_recv() {
                        match ev {
                            tray::TrayEvent::OpenSettings => {
                                settings_window::open_settings_window(cx);
                            }
                        }
                    }
                }) {
                    Ok(_) => {}
                    // 视图已释放则退出循环。
                    Err(_) => break,
                }
                // 短暂让步，避免空转占用 CPU；有事件时 try_recv 立即命中。
                cx.background_executor()
                    .timer(Duration::from_millis(2))
                    .await;
            }
        })
        .detach();

        Self {
            layout: standard_layout(),
            key_rx,
            tray_rx,
            _bounds_subscription: bounds_subscription,
            _global_subscription: global_subscription,
        }
    }

    /// 渲染一个键帽。
    fn render_key(
        &self,
        def: &KeyDef,
        pressed: bool,
        dark: bool,
        font_size: gpui_kit::Pixels,
    ) -> gpui_kit::Div {
        let (key_bg, key_border, text_col) = if dark {
            if pressed {
                (rgba(0x3b82f6ff), rgba(0x888888ff), rgba(0xffffffff))
            } else {
                (rgba(0x2a2a2eff), rgba(0x555555ff), rgba(0xffffffff))
            }
        } else if pressed {
            (rgba(0x3b82f6ff), rgba(0x1d4ed8ff), rgba(0xffffffff))
        } else {
            (rgba(0xffffffff), rgba(0x00000030), rgba(0x000000ff))
        };

        let mut key = div()
            .absolute()
            .rounded_md()
            .bg(key_bg)
            .border_1()
            .border_color(key_border)
            .flex()
            .items_center()
            .justify_center()
            .text_color(text_col)
            .text_size(font_size);
        // 空标签（如空格键）不渲染文本子元素。
        if !def.label.is_empty() {
            key = key.child(def.label);
        }
        key
    }

    fn render_keyboard(
        &self,
        settings: &state::Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let dark = settings.dark;
        let simple = settings.simple_mode;

        // 计算键帽尺寸：unit 由【高度】决定（键盘总高 6u）。
        // 这样主键盘区（左边）的键位大小只随窗口高度变化，
        // 简洁模式切换只增减右侧宽度，左边键位大小位置完全不动。
        let viewport = window.viewport_size();
        let unit = unit_from_height(viewport.height.as_f32());
        let font_size = font_size_for(unit);

        let mut container = div().w_full().h_full().relative();

        let pressed_set = &state(cx).pressed;
        let hidden = &settings.hidden_keys;
        for def in &self.layout.keys {
            // 简洁模式：只显示主键盘区，隐藏右侧两大块
            // （数字键盘 + 编辑区/方向键/顶部键）。
            if simple && (def.numpad || def.nav) {
                continue;
            }
            // 用户在键位设置里手动隐藏的键位：不渲染。
            if hidden.contains(&def.vk) {
                continue;
            }
            let pressed = pressed_set.contains(&(def.vk as u32));
            let x = PADDING + def.x * unit;
            let y = PADDING + def.row as f32 * unit;
            let w = def.w * unit - GAP;
            let h = def.h * unit - GAP;

            let key = self
                .render_key(def, pressed, dark, px(font_size))
                .top(px(y))
                .left(px(x))
                .w(px(w))
                .h(px(h));

            container = container.child(key);
        }

        container
    }
}

impl Render for KeyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = state(cx).settings.clone();
        let dark = settings.dark;

        // 简洁模式（或其它影响宽度的设置）改变后，同步窗口宽度。
        // 这里以高度为基准重算应有宽度，若不一致则 resize（resize 会再触发一次
        // observe_window_bounds，但宽度对齐后即收敛，不会死循环）。
        // 设置面板打开期间跳过：gpui 的 resize 会把键盘提到最前压住设置面板；
        // 面板关闭后 settings_open 变 false 触发本函数重绘，此时再同步。
        let h = window.bounds().size.height.as_f32();
        let w = window.bounds().size.width.as_f32();
        let (wa_w, wa_h) = work_area(window, cx);
        let (expected_w, expected_h) = desired_size(h, settings.simple_mode, wa_w, wa_h);
        if !state(cx).settings_open
            && ((w - expected_w).abs() > SIZE_EPS || (h - expected_h).abs() > SIZE_EPS)
        {
            window.resize(Size {
                width: px(expected_w),
                height: px(expected_h),
            });
        }

        // 背景色：透明模式下不画底板（窗口级透明透出下层），
        // 不透明模式下画一个带圆角的主题色底板（完全不透明，有明确背景）。
        let bg = if settings.transparent {
            rgba(0x00000000)
        } else if dark {
            rgba(0x1e1e1eff)
        } else {
            rgba(0xffffffff)
        };

        // 根容器：整个窗口都是可拖动 + 右键菜单区域。
        // 注意顺序：先绑定鼠标事件，再包 context_menu，最后添加子元素。
        let root = div()
            .w_full()
            .h_full()
            .relative()
            .id("root")
            .rounded_lg()
            .bg(bg)
            .on_mouse_down(MouseButton::Left, |_e, window, cx| {
                let st = state_mut(cx);
                st.dragging = true;
                // 用 Win32 物理像素记录拖动起点：鼠标全局位置 + 窗口左上角。
                // GetCursorPos / GetWindowRect 都在同一虚拟屏幕物理坐标系，
                // 跨显示器、跨不同 DPI 缩放都能正确还原，无需 scale_factor 换算。
                if let Some(hwnd) = win32::hwnd_from(window) {
                    let cur = win32::cursor_pos().unwrap_or((0, 0));
                    let orig = win32::window_origin(hwnd).unwrap_or((0, 0));
                    st.drag = Some(state::DragState {
                        start_cursor: cur,
                        start_origin: orig,
                    });
                }
            })
            .on_mouse_move(|_e, window, cx| {
                let st = state(cx);
                if !st.dragging {
                    return;
                }
                if let Some(drag) = st.drag {
                    if let Some(hwnd) = win32::hwnd_from(window) {
                        if let Some(cur) = win32::cursor_pos() {
                            // 纯物理像素位移：new = 起点 + (当前鼠标 - 起点鼠标)。
                            // 全程不引入逻辑像素与 scale_factor，因此跨屏/混合 DPI 下
                            // 窗口严格跟随鼠标，不会再卡在屏幕边界或飞出屏幕外。
                            let dx = cur.0 - drag.start_cursor.0;
                            let dy = cur.1 - drag.start_cursor.1;
                            let new_left = drag.start_origin.0 + dx;
                            let new_top = drag.start_origin.1 + dy;
                            win32::move_window(hwnd, new_left, new_top);
                        }
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, |_e, _window, cx| {
                let st = state_mut(cx);
                st.dragging = false;
                st.drag = None;
            })
            .on_scroll_wheel(|event, window, cx| {
                // 设置面板打开期间不改窗口尺寸：gpui 内部 resize 会顺带把键盘窗口
                // 提到最前并抢焦点，压住设置面板（与自动同步宽度同一条规则）。
                if state(cx).settings_open {
                    return;
                }
                // Windows 平台把 WM_MOUSEWHEEL 转成 Lines：一格 ≈ 3 行
                // （`wheel_scroll_lines` 默认值）。取垂直分量换算成"档数"，向上滚 = 放大。
                let ticks = match event.delta {
                    gpui_kit::ScrollDelta::Lines(p) => p.y / 3.0,
                    gpui_kit::ScrollDelta::Pixels(p) => p.y.as_f32() / 40.0,
                };
                if ticks == 0.0 {
                    return;
                }
                let simple = state(cx).settings.simple_mode;
                let old_w = window.bounds().size.width.as_f32();
                let old_h = window.bounds().size.height.as_f32();
                // 以键帽单位做乘性缩放：每档变化比例一致，手感线性，
                // 不会出现"键盘越大、每档跳得越多"的突兀感。
                let unit = unit_from_height(old_h);
                let new_unit = unit * (1.0 + ZOOM_PER_NOTCH * ticks);
                let want_h = PADDING * 2.0 + EXTENT_H * new_unit;
                let (wa_w, wa_h) = work_area(window, cx);
                let (w, h) = desired_size(want_h, simple, wa_w, wa_h);
                // 已经顶到上下限时不再 resize，避免无意义的重设尺寸。
                if (w - old_w).abs() < SIZE_EPS && (h - old_h).abs() < SIZE_EPS {
                    return;
                }
                window.resize(Size {
                    width: px(w),
                    height: px(h),
                });
                // 记进全局状态：设置面板的键盘预览按该高度等比绘制；同时持久化，
                // 下次启动直接沿用用户调好的大小。
                {
                    let st = state_mut(cx);
                    st.settings.keyboard_height = h;
                    st.main_keyboard_height = h;
                }
                state::save_persisted(&state(cx).settings);
                window.refresh();
            })
            .context_menu({
                let passthrough = settings.mouse_passthrough;
                let transparent = settings.transparent;
                let simple = settings.simple_mode;
                let dark = settings.dark;
                move |menu, _window, _cx| build_menu(menu, passthrough, transparent, simple, dark)
            });

        // 渲染键盘。
        let keyboard = self.render_keyboard(&settings, window, cx);
        root.child(keyboard)
    }
}

/// 构建右键菜单。
fn build_menu(
    menu: PopupMenu,
    passthrough: bool,
    transparent: bool,
    simple: bool,
    dark: bool,
) -> PopupMenu {
    menu.item(
        // 面板入口放最上面：它是"进设置"的统一入口，和托盘菜单里的
        // 「打开设置面板」是同一个动作（open_settings_window 内部会去重，
        // 已打开则只把已有窗口激活到前台）。
        PopupMenuItem::new("打开设置面板").on_click(|_e, _window, cx| {
            settings_window::open_settings_window(cx);
        }),
    )
    .separator()
    .item(
        PopupMenuItem::new("鼠标穿透")
            .checked(passthrough)
            .on_click(|_e, window, cx| {
                let s = &mut state_mut(cx).settings;
                s.mouse_passthrough = !s.mouse_passthrough;
                let v = s.mouse_passthrough;
                tray::set_passthrough(v);
                if let Some(hwnd) = win32::hwnd_from(window) {
                    win32::set_mouse_passthrough(hwnd, v);
                }
                window.refresh();
            }),
    )
    .item(
        PopupMenuItem::new("背景透明")
            .checked(transparent)
            .on_click(|_e, window, cx| {
                let s = &mut state_mut(cx).settings;
                s.transparent = !s.transparent;
                // 背景透明/不透明完全由 view 层 `.bg()` 控制，
                // 只需触发重绘即可，无需 Win32 层操作。
                window.refresh();
            }),
    )
    .item(
        PopupMenuItem::new("简洁模式")
            .checked(simple)
            .on_click(|_e, window, cx| {
                let simple = {
                    let s = &mut state_mut(cx).settings;
                    s.simple_mode = !s.simple_mode;
                    s.simple_mode
                };
                // 切换简洁模式：保持高度不变，只重算宽度。
                // 开启 → 只留主键盘区（隐藏数字键盘 + 编辑区/方向键/顶部键）；
                // 关闭 → 右侧两大块全部恢复。
                let h = window.bounds().size.height.as_f32();
                let (wa_w, wa_h) = work_area(window, cx);
                let (new_w, new_h) = desired_size(h, simple, wa_w, wa_h);
                window.resize(Size {
                    width: px(new_w),
                    height: px(new_h),
                });
                window.refresh();
            }),
    )
    .separator()
    .item(
        PopupMenuItem::new("切换主题")
            .checked(dark)
            .on_click(|_e, window, cx| {
                let new_dark = {
                    let s = &mut state_mut(cx).settings;
                    s.dark = !s.dark;
                    s.dark
                };
                let mode = if new_dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                };
                Theme::change(mode, None, cx);
                window.refresh();
            }),
    )
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
        // 只初始化 theme（menu 渲染需要 ActiveTheme 全局）。
        // 注意：
        // 1. 不使用 gpui_kit::open_window（它会用 base::Root 包裹 root surface），
        //    避免 root surface 被注入白色背景。
        // 2. 不调用 gpui_kit::component::init(cx)——它会初始化 table/tree/editor 等全部组件，
        //    把大量无用代码链接进二进制，导致体积膨胀。这里只需 theme。
        gpui_kit::component::theme::init(cx);

        // 初始尺寸按持久化的键盘高度还原（用户上次用滚轮调过大小就一直保持）。
        // 这里还没有窗口，拿不到显示器工作区，先不按屏幕收窄；首帧 render 会用真实
        // 工作区再钳制一次，因此不会真的超出屏幕。
        let init_settings = state::load_persisted();
        let (init_w, init_h) = desired_size(
            init_settings.keyboard_height,
            init_settings.simple_mode,
            f32::INFINITY,
            f32::INFINITY,
        );

        let options = WindowOptions {
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("show-your-key".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_background: gpui_kit::WindowBackgroundAppearance::Transparent,
            is_resizable: true,
            window_min_size: Some(Size {
                width: px(MIN_WINDOW_W),
                height: px(MIN_WINDOW_H),
            }),
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::centered(
                None,
                Size {
                    // 默认 unit ≈ 36px：高 = 24 + 6*36 = 240，宽按当前模式计算。
                    width: px(init_w),
                    height: px(init_h),
                },
                cx,
            ))),
            ..Default::default()
        };

        let window = cx
            .open_window(options, |window, cx| cx.new(|cx| KeyView::new(window, cx)))
            .expect("failed to open window");

        // 应用窗口外观与初始设置：置顶、去边框阴影、图标、透明、鼠标穿透。
        window
            .update(cx, |_, window, cx| {
                if let Some(hwnd) = win32::hwnd_from(window) {
                    win32::set_main_hwnd(hwnd);
                    win32::set_always_on_top(hwnd, true);
                    win32::remove_border_and_shadow(hwnd);
                    win32::hide_from_taskbar(hwnd);
                    win32::set_window_icon(hwnd);
                    let s = state(cx).settings.clone();
                    // 背景透明/不透明完全由 view 层 `.bg()` 控制（见 render），
                    // 窗口层 `window_background: Transparent` 恒定，无需 Win32 操作。
                    win32::set_mouse_passthrough(hwnd, s.mouse_passthrough);
                    // 启动托盘图标。
                    tray::start(hwnd);
                }
            })
            .ok();
    });
}
