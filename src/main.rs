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

mod keyboard;
mod keyhook;
mod state;
mod tray;
mod win32;

use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui::{
    div, px, rgba, AppContext, Bounds, Context, ParentElement, Point, Render, Size, Styled,
    Window, WindowOptions, InteractiveElement, IntoElement, MouseButton,
};
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::theme::{Theme, ThemeMode};

use crate::keyboard::{standard_layout, KeyDef, Layout};
use crate::state::{state, state_mut, AppState};

/// 主键盘区逻辑宽度（单位 u，0..15，简洁模式）。
const SIMPLE_EXTENT_W: f32 = 15.0;
/// 全键盘逻辑宽度（单位 u，0..22.9，含编辑区/方向键/顶部键 + 数字键盘）。
const NUMPAD_EXTENT_W: f32 = 22.9;
/// 键盘逻辑高度（单位 u，6 行）。
const EXTENT_H: f32 = 6.0;

/// 窗口内边距（像素，固定值，不随窗口缩放）。
const PADDING: f32 = 12.0;
/// 键帽之间的间隙（像素，固定值）。
const GAP: f32 = 3.0;

/// 由窗口高度计算键帽单位（u → 像素）。
/// 高度 = 2*PADDING + EXTENT_H*unit，反解 unit。
fn unit_from_height(h: f32) -> f32 {
    ((h - PADDING * 2.0) / EXTENT_H).max(1.0)
}

/// 由键帽单位和显示模式，计算窗口应有的宽度。
/// 宽度 = 2*PADDING + extent_w*unit。
/// 简洁模式 = 只留主键盘区（15u）；否则 = 全键盘（含数字键盘，22.9u）。
fn width_for(unit: f32, simple: bool) -> f32 {
    let extent_w = if simple { SIMPLE_EXTENT_W } else { NUMPAD_EXTENT_W };
    PADDING * 2.0 + extent_w * unit
}

/// 根视图。
struct KeyView {
    layout: Layout,
    /// 全局按键事件接收端。
    key_rx: Receiver<keyhook::KeyEvent>,
    /// 窗口尺寸变化订阅（必须持有，否则观察器被丢弃）。
    _bounds_subscription: gpui::Subscription,
}

impl KeyView {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_global(AppState::new());
        let key_rx = keyhook::start();

        // 监听窗口尺寸变化，保持键盘比例（以高度为基准）。
        let bounds_subscription = cx.observe_window_bounds(_window, |_view, window, cx| {
            let s = &state(cx).settings;
            let bounds = window.bounds();
            let w = bounds.size.width.as_f32();
            let h = bounds.size.height.as_f32();
            // 以【高度】为基准：unit 由高度决定，宽度由 unit 和显示模式决定。
            // 这样主键盘区（左边）大小只随高度变化，右侧区域增减只改变宽度。
            let unit = unit_from_height(h);
            let expected_w = width_for(unit, s.simple_mode);
            if (w - expected_w).abs() > 0.5 {
                window.resize(Size {
                    width: px(expected_w),
                    height: px(h),
                });
            }
            cx.notify();
        });

        // 关键：用独立异步循环持续读取全局按键事件并刷新 UI。
        // 不能依赖 render() 被触发——窗口失焦且无变化时 render() 根本不执行，
        // 导致非聚焦输入不亮、抬起后不灭。
        cx.spawn(async move |this: gpui::WeakEntity<KeyView>, cx: &mut gpui::AsyncApp| {
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
            _bounds_subscription: bounds_subscription,
        }
    }

    /// 渲染一个键帽。
    fn render_key(
        &self,
        def: &KeyDef,
        pressed: bool,
        dark: bool,
        font_size: gpui::Pixels,
    ) -> gpui::Div {
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
    ) -> gpui::Div {
        let dark = settings.dark;
        let simple = settings.simple_mode;

        // 计算键帽尺寸：unit 由【高度】决定（键盘总高 6u）。
        // 这样主键盘区（左边）的键位大小只随窗口高度变化，
        // 简洁模式切换只增减右侧宽度，左边键位大小位置完全不动。
        let viewport = window.viewport_size();
        let unit = unit_from_height(viewport.height.as_f32());
        let font_size = (unit * 0.24).max(7.0);

        let mut container = div().w_full().h_full().relative();

        let pressed_set = &state(cx).pressed;
        for def in &self.layout.keys {
            // 简洁模式：只显示主键盘区，隐藏右侧两大块
            // （数字键盘 + 编辑区/方向键/顶部键）。
            if simple && (def.numpad || def.nav) {
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
                // mouse_position() 返回窗口内坐标，加上窗口原点得到全局坐标。
                let origin = window.bounds().origin;
                let global_mouse = Point {
                    x: px(window.mouse_position().x.as_f32() + origin.x.as_f32()),
                    y: px(window.mouse_position().y.as_f32() + origin.y.as_f32()),
                };
                st.drag = Some(state::DragState {
                    start_mouse: global_mouse,
                    start_origin: origin,
                });
            })
            .on_mouse_move(|_e, window, cx| {
                let st = state(cx);
                if !st.dragging {
                    return;
                }
                if let Some(drag) = st.drag {
                    let origin = window.bounds().origin;
                    let cur_global = Point {
                        x: px(window.mouse_position().x.as_f32() + origin.x.as_f32()),
                        y: px(window.mouse_position().y.as_f32() + origin.y.as_f32()),
                    };
                    let dx = cur_global.x.as_f32() - drag.start_mouse.x.as_f32();
                    let dy = cur_global.y.as_f32() - drag.start_mouse.y.as_f32();
                    let new_x = drag.start_origin.x.as_f32() + dx;
                    let new_y = drag.start_origin.y.as_f32() + dy;
                    if let Some(hwnd) = win32::hwnd_from(window) {
                        // bounds()/mouse_position() 为逻辑像素，SetWindowPos 需要物理像素。
                        let scale = window.scale_factor();
                        win32::move_window(hwnd, (new_x * scale) as i32, (new_y * scale) as i32);
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, |_e, _window, cx| {
                let st = state_mut(cx);
                st.dragging = false;
                st.drag = None;
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
                let s = &mut state_mut(cx).settings;
                s.simple_mode = !s.simple_mode;
                // 切换简洁模式：保持高度不变，只重算宽度。
                // 开启 → 只留主键盘区（隐藏数字键盘 + 编辑区/方向键/顶部键）；
                // 关闭 → 右侧两大块全部恢复。
                let h = window.bounds().size.height.as_f32();
                let unit = unit_from_height(h);
                let new_w = width_for(unit, s.simple_mode);
                window.resize(Size {
                    width: px(new_w),
                    height: px(h),
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
    gpui_platform::application().run(|cx| {
        // 只初始化 theme（menu 渲染需要 ActiveTheme 全局）。
        // 注意：
        // 1. 不使用 gpui_kit::open_window，避免 root surface 被注入白色背景。
        // 2. 不调用 gpui_component::init(cx)——它会初始化 table/tree/editor 等全部组件，
        //    把大量无用代码链接进二进制，导致体积膨胀。这里只需 theme。
        gpui_component::theme::init(cx);

        let options = WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("show-your-key".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            is_resizable: true,
            window_min_size: Some(Size {
                width: px(300.),
                height: px(120.),
            }),
            window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
                None,
                Size {
                    // 初始 unit ≈ 36px：高 = 24 + 6*36 = 240，宽按默认（非简洁 + 显示数字键盘）计算。
                    width: px(width_for(36.0, false)),
                    height: px(2.0 * PADDING + EXTENT_H * 36.0),
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
