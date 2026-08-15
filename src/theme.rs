use gpui::{App, px, rgb};
use gpui_component::{Theme, ThemeTokens};

/// Warm charcoal launcher palette. Surfaces lift in small steps. Primary
/// actions are a white pill. Running state uses moss, not neon.
pub fn apply_launcher_colors(cx: &mut App) {
    let theme = Theme::global_mut(cx);

    let bg = rgb(0x121110).into();
    let raised = rgb(0x1b1a18).into();
    let surface = rgb(0x252421).into();
    let surface_hover = rgb(0x2e2c28).into();
    let surface_active = rgb(0x211f1c).into();
    let border = rgb(0x33312c).into();
    let fg = rgb(0xeceae4).into();
    let muted_fg = rgb(0x9a968c).into();
    let white = rgb(0xf4f3ef).into();
    let ink = rgb(0x161512).into();
    let moss = rgb(0x6f8f6a).into();
    let moss_fg = rgb(0x121411).into();

    theme.colors.background = bg;
    theme.sidebar = bg;
    theme.title_bar = bg;
    theme.status_bar = bg;
    theme.popover = raised;
    theme.accordion = bg;
    theme.group_box = raised;
    theme.colors.list = bg;
    theme.table = bg;
    theme.tiles = raised;
    theme.tab = bg;
    theme.tab_bar = bg;
    theme.tab_bar_segmented = surface;
    theme.tab_active = raised;

    theme.foreground = fg;
    theme.sidebar_foreground = fg;
    theme.popover_foreground = fg;
    theme.tab_foreground = muted_fg;
    theme.tab_active_foreground = fg;
    theme.group_box_foreground = fg;
    theme.sidebar_accent_foreground = fg;

    theme.muted = surface;
    theme.muted_foreground = muted_fg;
    theme.secondary = surface;
    theme.secondary_hover = surface_hover;
    theme.secondary_active = surface_active;
    theme.secondary_foreground = fg;
    theme.accent = surface_hover;
    theme.accent_foreground = fg;
    theme.sidebar_accent = surface;
    theme.skeleton = surface;

    theme.border = border;
    theme.sidebar_border = border;
    theme.input = border;
    theme.title_bar_border = border;
    theme.status_bar_border = border;
    theme.table_row_border = border;
    theme.window_border = border;
    theme.ring = rgb(0x4a4740).into();

    theme.primary = white;
    theme.primary_hover = rgb(0xffffff).into();
    theme.primary_active = rgb(0xdddcd6).into();
    theme.primary_foreground = ink;
    theme.button_primary = white;
    theme.button_primary_hover = rgb(0xffffff).into();
    theme.button_primary_active = rgb(0xdddcd6).into();
    theme.button_primary_foreground = ink;
    theme.sidebar_primary = white;
    theme.sidebar_primary_foreground = ink;

    theme.button = surface;
    theme.button_hover = surface_hover;
    theme.button_active = surface_active;
    theme.button_foreground = fg;
    theme.button_secondary = surface;
    theme.button_secondary_hover = surface_hover;
    theme.button_secondary_active = surface_active;
    theme.button_secondary_foreground = fg;

    theme.list_active = surface;
    theme.list_active_border = surface;
    theme.list_hover = surface;
    theme.list_even = bg;
    theme.list_head = raised;
    theme.list.active_highlight = false;

    theme.table_active = surface;
    theme.table_active_border = surface;
    theme.table_hover = surface;
    theme.table_even = bg;
    theme.table_head = raised;

    theme.success = moss;
    theme.success_hover = rgb(0x7d9d78).into();
    theme.success_active = rgb(0x617e5c).into();
    theme.success_foreground = moss_fg;

    let clay = rgb(0xc47a6a);
    theme.danger = clay.into();
    theme.danger_hover = rgb(0xd18b7c).into();
    theme.danger_active = rgb(0xb36b5d).into();
    theme.danger_foreground = rgb(0x1a1210).into();

    let dust = rgb(0xc4a46a);
    theme.warning = dust.into();
    theme.warning_hover = rgb(0xd4b57a).into();
    theme.warning_active = rgb(0xb09058).into();
    theme.warning_foreground = ink;

    let steel = rgb(0x7a8a96);
    theme.info = steel.into();
    theme.info_hover = rgb(0x8b9aa5).into();
    theme.info_active = rgb(0x687882).into();
    theme.info_foreground = ink;

    theme.selection = rgb(0x3d3b36).into();
    theme.progress_bar = white;
    theme.slider_bar = white;
    theme.slider_thumb = ink;
    theme.caret = fg;
    theme.link = fg;
    theme.link_hover = white;
    theme.link_active = muted_fg;
    theme.overlay = rgb(0x0a0908).into();
    theme.drag_border = rgb(0x4a4740).into();
    theme.font_family = ".SystemUIFont".into();
    theme.radius = px(8.);
    theme.radius_lg = px(12.);
    theme.tokens = ThemeTokens::from(&theme.colors);
}
