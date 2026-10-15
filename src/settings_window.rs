//! 设置面板窗口（fluent 风格）。
//!
//! 布局：左侧导航栏（常规 / 外观 / 键位）+ 右侧内容区。
//! 所有设置都读写全局 `AppState`，切换后主窗口通过 `observe_global` 自动同步。
//!
//! ⚠️ 重要（gpui 布局陷阱）：
//! 窗口的根元素只有在尺寸为 **auto** 时才会被 gpui 拉伸到整个窗口
//! （见 gpui `window.rs::draw_roots` → `stretch_auto_size_to_fill`）。
//! 所以 `SettingsView::render` 返回的最外层 `div` **不能**写 `.w_full()/.h_full()`，
//! 否则宽高只有内容宽度（≈左侧栏宽度），右侧内容区宽度会退化成 0 而完全不可见。

use gpui_kit::component::switch::Switch;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    div, px, App, AppContext, Context, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, WindowHandle,
};

use crate::keyboard::standard_layout;
use crate::state::{self, state, state_mut, SettingsPage};

/// 键盘预览容器左右外边距（对应 `.mx_5()`）。
const PREVIEW_MARGIN_X: f32 = 20.0;
/// 左侧导航栏宽度（像素）。
const SIDEBAR_WIDTH: f32 = 200.0;

/// 切换某个键位的显示/隐藏，并落盘。
///
/// `remove` 返回 true 表示原本处于隐藏状态（已恢复显示），否则插入隐藏。
fn toggle_key_visible(vk: u16, cx: &mut App) {
    let st = state_mut(cx);
    if !st.settings.hidden_keys.remove(&vk) {
        st.settings.hidden_keys.insert(vk);
    }
    state::save_persisted(&st.settings);
}

/// 设置面板根视图。
pub struct SettingsView {
    /// 观察全局状态，设置变化时刷新（必须持有，否则订阅被丢弃）。
    _sub: gpui_kit::Subscription,
    /// 面板激活/失活订阅（必须持有）。
    _activation_sub: gpui_kit::Subscription,
    /// 面板关闭时的收尾（必须持有）。
    _release_sub: gpui_kit::Subscription,
}

/// 主键盘窗口是常驻"始终置顶"的，若不处理，设置面板一打开就会被键盘压住
/// （非置顶窗口永远不可能盖在置顶窗口上面）。用户明确要求设置面板本身不置顶，
/// 所以这里改成：**面板在前台时把主键盘降为普通窗口，面板一失活立刻恢复置顶。**
///
/// 判据取"面板是否在前台"而不是"面板是否还开着"——原因见 `SettingsView::new` 里
/// 激活观察器的注释。
///
/// 注意主键盘窗口**不能**在所有情况下都保持置顶：设置面板打开后如果键盘仍然置顶，
/// 它会一直压住面板（非置顶窗口永远盖不过置顶窗口），面板等于不可用。
fn set_keyboard_topmost(on_top: bool) {
    if crate::win32::has_main_hwnd() {
        crate::win32::set_always_on_top(crate::win32::main_hwnd(), on_top);
    }
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe_global::<state::AppState>(|_this, cx| cx.notify());

        // 键盘窗口的置顶跟随「面板是否在**前台**」，而不是「面板是否**开着**」。
        //
        // 面板是普通窗口，只要键盘还是置顶的就永远压在它上面，没法操作，所以
        // 面板在前台时键盘必须让位。但判据不能是"面板还活着"——面板被别的窗口
        // 盖住时它依然活着，只按生命周期判断的话键盘会永久保持非置顶，用户看到
        // 的就是"键盘怎么不置顶了"，而且完全找不到原因（面板藏在别的窗口后面）。
        //
        // 所以改成事件驱动：面板被激活 → 键盘让位；面板一失活（用户点了别的
        // 窗口、面板被压到后面）→ 立刻恢复键盘置顶。
        let activation_sub = cx.observe_window_activation(window, |_this, window, _cx| {
            set_keyboard_topmost(!window.is_window_active());
        });

        let release_sub = cx.on_release(|_this, cx| {
            // 面板关闭：恢复主键盘置顶，并让主键盘重新同步宽度。
            state_mut(cx).settings_open = false;
            set_keyboard_topmost(true);
        });
        Self {
            _sub: sub,
            _activation_sub: activation_sub,
            _release_sub: release_sub,
        }
    }

    /// 渲染左侧导航栏。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let theme = cx.theme().clone();
        let current = state(cx).settings_page;

        let nav_items: [(&str, SettingsPage); 3] = [
            ("常规", SettingsPage::General),
            ("外观", SettingsPage::Appearance),
            ("键位", SettingsPage::Keys),
        ];

        let mut sidebar = div()
            .w(px(SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.colors.sidebar)
            .border_r_1()
            .border_color(theme.colors.sidebar_border)
            .py_2()
            .gap_1();

        for (label, page) in nav_items {
            let active = current == page;
            let bg = if active {
                theme.colors.sidebar_accent
            } else {
                gpui_kit::transparent_black()
            };
            let fg = if active {
                theme.colors.sidebar_accent_foreground
            } else {
                theme.colors.sidebar_foreground
            };
            let item = div()
                .mx_2()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(bg)
                .text_color(fg)
                .text_size(px(14.))
                .cursor_pointer()
                .child(label)
                // 这里拿到的是 &mut App（不是 Context<Self>），无法直接 notify；
                // 但修改全局 AppState 会触发 NotifyGlobalObservers，
                // SettingsView 的 observe_global 会自动重绘。
                .on_mouse_down(MouseButton::Left, move |_e, _window, cx| {
                    state_mut(cx).settings_page = page;
                });
            sidebar = sidebar.child(item);
        }

        sidebar
    }

    /// 渲染一个设置开关行（标题 + 副标题 + 右侧 Switch）。
    fn render_switch_row(
        id: &'static str,
        title: &str,
        subtitle: &str,
        checked: bool,
        on_change: impl Fn(bool, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let theme = cx.theme().clone();

        div()
            .w_full()
            .px_5()
            .py_4()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .border_b_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_color(theme.colors.foreground)
                            .text_size(px(14.))
                            .child(title.to_string()),
                    )
                    .child(
                        div()
                            .text_color(theme.colors.muted_foreground)
                            .text_size(px(12.))
                            .child(subtitle.to_string()),
                    ),
            )
            .child(
                Switch::new(id)
                    .checked(checked)
                    .on_click(move |new_val: &bool, _window, cx| {
                        on_change(*new_val, cx);
                    }),
            )
    }

    /// 页面标题。
    fn render_page_title(title: &str, cx: &mut Context<Self>) -> gpui_kit::Div {
        let theme = cx.theme().clone();
        div()
            .px_5()
            .pt_5()
            .pb_3()
            .text_color(theme.colors.foreground)
            .text_size(px(18.))
            .font_weight(gpui_kit::FontWeight::BOLD)
            .child(title.to_string())
    }

    /// 渲染常规设置页。
    fn render_general(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let s = state(cx).settings.clone();

        let mut page = div().w_full().flex().flex_col();
        page = page.child(Self::render_page_title("常规", cx));

        page = page.child(Self::render_switch_row(
            "row-autostart",
            "开机自启",
            "登录 Windows 后自动运行 show-your-key",
            s.autostart,
            |v, cx| {
                crate::autostart::set_autostart(v);
                state_mut(cx).settings.autostart = v;
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        page = page.child(Self::render_switch_row(
            "row-passthrough",
            "鼠标穿透",
            "鼠标点击将穿透窗口，落到下层应用",
            s.mouse_passthrough,
            |v, cx| {
                state_mut(cx).settings.mouse_passthrough = v;
                let hwnd = crate::win32::main_hwnd();
                crate::win32::set_mouse_passthrough(hwnd, v);
                crate::tray::set_passthrough(v);
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        page
    }

    /// 渲染外观设置页。
    fn render_appearance(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let s = state(cx).settings.clone();

        let mut page = div().w_full().flex().flex_col();
        page = page.child(Self::render_page_title("外观", cx));

        page = page.child(Self::render_switch_row(
            "row-transparent",
            "背景透明",
            "不绘制背景底板，仅显示键帽",
            s.transparent,
            |v, cx| {
                state_mut(cx).settings.transparent = v;
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        page = page.child(Self::render_switch_row(
            "row-keycap-transparent",
            "键位透明背景",
            "每个键帽不绘制背景，只保留边框与文字",
            s.keycap_transparent,
            |v, cx| {
                state_mut(cx).settings.keycap_transparent = v;
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        page = page.child(Self::render_switch_row(
            "row-dark",
            "暗色主题",
            "在浅色与暗色主题之间切换",
            s.dark,
            |v, cx| {
                state_mut(cx).settings.dark = v;
                let mode = if v { ThemeMode::Dark } else { ThemeMode::Light };
                Theme::change(mode, None, cx);
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        page
    }

    /// 渲染键位设置页：简洁模式开关 + 全键盘预览。
    ///
    /// 预览与屏幕上真实键盘**等比**：键帽单位由主键盘窗口当前高度反解
    /// （与 `main.rs::unit_from_height` 同一公式），所以两边看起来一样大；
    /// 若设置窗口太窄，再按可用宽度夹一次，保证预览不会被裁掉。
    fn render_keys(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let s = state(cx).settings.clone();
        let theme = cx.theme().clone();
        let layout = standard_layout();

        // 与屏幕键盘一致的目标单位。
        let target_unit = crate::unit_from_height(state(cx).main_keyboard_height);
        // 可用宽度 → 允许的最大单位。
        let viewport_w = window.viewport_size().width.as_f32();
        let avail_w = (viewport_w - SIDEBAR_WIDTH - PREVIEW_MARGIN_X * 2.0).max(120.0);
        let fit_unit = ((avail_w - crate::PADDING * 2.0) / crate::NUMPAD_EXTENT_W).max(4.0);
        let unit = target_unit.min(fit_unit);
        let font_size = crate::font_size_for(unit);

        // 键盘预览整体尺寸（含数字键盘与导航区，最宽 22.9u）。
        let kb_w = crate::PADDING * 2.0 + crate::NUMPAD_EXTENT_W * unit;
        let kb_h = crate::PADDING * 2.0 + crate::EXTENT_H * unit;

        let mut page = div().w_full().flex().flex_col();
        page = page.child(Self::render_page_title("键位", cx));

        page = page.child(Self::render_switch_row(
            "row-simple",
            "简洁模式",
            "隐藏数字键盘、编辑区、方向键与顶部键",
            s.simple_mode,
            |v, cx| {
                state_mut(cx).settings.simple_mode = v;
                state::save_persisted(&state(cx).settings);
            },
            cx,
        ));

        // 提示文字。
        page = page.child(
            div()
                .px_5()
                .pt_3()
                .pb_2()
                .text_color(theme.colors.muted_foreground)
                .text_size(px(12.))
                .child("点击键位可切换其是否显示；按住 Ctrl 划过的键位会依次切换。置灰表示该键位不会显示在键盘上。"),
        );

        // 键盘预览容器。
        let mut canvas = div().relative().w(px(kb_w)).h(px(kb_h));

        // 绘制所有键位。
        for def in &layout.keys {
            let hidden = s.hidden_keys.contains(&def.vk);
            let x = crate::PADDING + def.x * unit;
            let y = crate::PADDING + def.row as f32 * unit;
            let w = def.w * unit - crate::GAP;
            let h = def.h * unit - crate::GAP;

            // 置灰样式：隐藏的键位使用弱化颜色。
            let (bg, border, fg) = if hidden {
                (
                    theme.colors.muted,
                    theme.colors.border,
                    theme.colors.muted_foreground,
                )
            } else {
                (
                    theme.colors.popover,
                    theme.colors.border,
                    theme.colors.foreground,
                )
            };

            let vk = def.vk;
            let label = def.label.to_string();
            let mut key = div()
                // 必须有稳定 id：gpui 的 hover 监听器状态是挂在 element_state 上的，
                // 没有 id 就没有 element_state，监听器根本不会注册（静默失效）。
                .id(gpui_kit::ElementId::named_usize("settings-key", vk as usize))
                .absolute()
                .top(px(y))
                .left(px(x))
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .bg(bg)
                .border_1()
                .border_color(border)
                .text_color(fg)
                .text_size(px(font_size))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                // 按住 Ctrl 划过即切换显隐。
                //
                // 触发用 `on_mouse_move`（只有鼠标**真的移动**才回调）：这样"划过"
                // 天然就是"移动进入一次、切换一次"，连续移动不会重复切换；
                // 而 `on_hover` 只在悬停状态变化时回调，会被鼠标按下/抬起的
                // 伪状态切换（gpui 在按住期间强制把 is_hovered 置 false）带偏，
                // 所以这里只让它干一件事：离开该键时清掉"已切换"标记，
                // 从而再次划回来还能继续切换。
                //
                // 标记用键位本身而不是布尔量：同一帧里"先进入 B、再离开 A"
                // 两种回调顺序下结果都一致。
                //
                // hover 必须用 `InputModalityIndependent`：默认的
                // `InputModalityAware` 在最后一次输入是键盘（刚按下 Ctrl 就是）时
                // `is_hovered` 恒为 false，离开检测会失效。
                .hover_listener_mode(gpui_kit::HoverListenerMode::InputModalityIndependent)
                .on_hover(move |hovered: &bool, _window, cx| {
                    if !*hovered && state(cx).ctrl_sweep_key == Some(vk) {
                        state_mut(cx).ctrl_sweep_key = None;
                    }
                })
                .on_mouse_move(move |_e, window, cx| {
                    if !window.modifiers().control {
                        // 没按住 Ctrl：清掉陈旧标记，这样之后再按住 Ctrl 划过
                        // 同一个键仍然能切换。
                        if state(cx).ctrl_sweep_key.is_some() {
                            state_mut(cx).ctrl_sweep_key = None;
                        }
                        return;
                    }
                    // 本次悬停已经切换过：等离开后再划回来才切换下一次。
                    if state(cx).ctrl_sweep_key == Some(vk) {
                        return;
                    }
                    state_mut(cx).ctrl_sweep_key = Some(vk);
                    toggle_key_visible(vk, cx);
                })
                .on_mouse_down(MouseButton::Left, move |_e, window, cx| {
                    // 按住 Ctrl 时该键已由"划过"逻辑切换过（或即将由移动触发），
                    // 点击不再重复切换，避免一次点击把状态又翻回去。
                    if window.modifiers().control {
                        return;
                    }
                    toggle_key_visible(vk, cx);
                });
            if !label.is_empty() {
                key = key.child(label);
            }
            canvas = canvas.child(key);
        }

        page = page.child(
            div()
                .mx_5()
                .mb_5()
                .rounded_lg()
                .border_1()
                .border_color(theme.colors.border)
                .bg(theme.colors.popover)
                .overflow_hidden()
                .child(canvas),
        );

        page
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let page = state(cx).settings_page;

        let content = match page {
            SettingsPage::General => self.render_general(cx),
            SettingsPage::Appearance => self.render_appearance(cx),
            SettingsPage::Keys => self.render_keys(window, cx),
        };
        let content = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.colors.background)
            .child(content);

        // 注意：
        // 1. 必须显式 `.flex()`——gpui 的 `flex_row()`/`flex_col()` 只设置 flex-direction，
        //    不会把 display 设成 flex（默认是 block）。少了它，`flex_1()` 不会分配宽度，
        //    右侧内容区宽度会退化成 0，看起来就是"设置面板里什么都没有"。
        // 2. 最外层不能写 `.w_full()/.h_full()`，必须保持 auto 让 gpui 拉伸到整个窗口。
        div()
            .flex()
            .flex_row()
            .bg(theme.colors.background)
            .child(self.render_sidebar(cx))
            .child(content)
    }
}

/// 已打开设置窗口的句柄（用于避免重复打开、聚焦已有窗口）。
static SETTINGS_HANDLE: std::sync::Mutex<Option<WindowHandle<SettingsView>>> =
    std::sync::Mutex::new(None);

/// 打开（或聚焦已打开的）设置面板窗口。
pub fn open_settings_window(cx: &mut App) {
    // 面板打开期间：主键盘暂停自适应宽度 + 取消置顶（否则键盘永远盖在面板上）。
    state_mut(cx).settings_open = true;
    set_keyboard_topmost(false);

    // 若已存在且仍有效，则激活并返回。
    if let Ok(guard) = SETTINGS_HANDLE.lock() {
        if let Some(handle) = guard.as_ref() {
            let activated = handle
                .update(cx, |_view, window, _cx| window.activate_window())
                .is_ok();
            if activated {
                return;
            }
        }
    }

    let options = gpui_kit::WindowOptions {
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: Some("show-your-key 设置".into()),
            appears_transparent: false,
            ..Default::default()
        }),
        // 尺寸要放得下与真实键盘等大的预览（预览本身会按可用宽度自适应）。
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds::centered(
            None,
            gpui_kit::Size {
                width: px(1160.),
                height: px(720.),
            },
            cx,
        ))),
        window_min_size: Some(gpui_kit::Size {
            width: px(720.),
            height: px(440.),
        }),
        ..Default::default()
    };

    let handle = cx
        .open_window(options, |window, cx| cx.new(|cx| SettingsView::new(window, cx)))
        .expect("failed to open settings window");

    // 不置顶（用户要求）：只激活到前台即可，之后跟随普通窗口的 z 序。
    // 注意主键盘窗口本身是"始终置顶"的，所以打开设置面板时它可能浮在
    // 设置面板上面，拖开它即可。
    let _ = handle.update(cx, |_view, window, _cx| {
        window.activate_window();
    });

    if let Ok(mut guard) = SETTINGS_HANDLE.lock() {
        *guard = Some(handle);
    }
}
