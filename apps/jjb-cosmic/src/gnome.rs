// SPDX-License-Identifier: GPL-3.0-only

//! GNOME look for the chrome when the app runs on GNOME.
//!
//! Still libcosmic, never GTK (see DECISIONS.md, 2026-11-25): the title-bar
//! widgets are restyled after libadwaita so the window sits next to GNOME's
//! own apps without standing out.
//!
//! - Window controls: libadwaita's round buttons, a 24 px circle in 10 % of
//!   the text colour (15 % hovered, 30 % pressed), in the text colour —
//!   libcosmic tints its own in the accent colour while the window is
//!   focused, and has no way to change that, so its controls are switched
//!   off (`core.window.show_*`) and these are drawn in `header_end`.
//! - Title-bar buttons: libadwaita's flat buttons, 34 px with a 6 px radius,
//!   7 % hovered, 16 % pressed; a toggled one is 10 % grey, not accent.
//! - Menus: libadwaita popovers, a 12 px radius, 6 px of padding round the
//!   items, its popover colour and faint outline; the menu titles are flat
//!   title-bar buttons.
//!
//! Elsewhere (COSMIC, KDE…) none of this applies and libcosmic draws as
//! it always has.

use crate::desktop::Desktop;
use cosmic::iced::{Background, Color, Length};
use cosmic::theme;
use cosmic::widget::{self, button, icon};
use cosmic::Element;

/// Whether the chrome should wear the GNOME look.
pub fn active() -> bool {
    Desktop::current() == Desktop::Gnome
}

fn fg(theme: &cosmic::Theme) -> Color {
    let cosmic = theme.cosmic();
    cosmic.background(theme.transparent).on.into()
}

fn tint(theme: &cosmic::Theme, alpha: f32) -> Color {
    Color {
        a: alpha,
        ..fg(theme)
    }
}

/// A button style from one background alpha per state (`None` = clear).
fn shaded(radius: f32, rest: Option<f32>, hover: f32, press: f32) -> theme::Button {
    shaded_with(radius, rest, hover, press, true)
}

/// `dim_disabled: false` for menu titles: the menu bar opens them itself,
/// so they have no press message and the button counts as disabled.
fn shaded_with(
    radius: f32,
    rest: Option<f32>,
    hover: f32,
    press: f32,
    dim_disabled: bool,
) -> theme::Button {
    let style = move |theme: &cosmic::Theme, alpha: Option<f32>, disabled: bool| {
        let mut s = button::Style::new();
        s.background = alpha.map(|a| Background::Color(tint(theme, a)));
        s.border_radius = radius.into();
        let mut colour = fg(theme);
        if disabled && dim_disabled {
            colour.a = 0.5;
        }
        s.icon_color = Some(colour);
        s.text_color = Some(colour);
        s
    };
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, rest, false)),
        disabled: Box::new(move |t| style(t, rest, true)),
        hovered: Box::new(move |_, t| style(t, Some(hover), false)),
        pressed: Box::new(move |_, t| style(t, Some(press), false)),
    }
}

/// libadwaita `button.flat` in a header bar; `checked` for a toggle that is on.
pub fn flat_button(checked: bool) -> theme::Button {
    if checked {
        shaded(6.0, Some(0.10), 0.13, 0.19)
    } else {
        shaded(6.0, None, 0.07, 0.16)
    }
}

/// An icon button in the title bar, GNOME-sized.
pub fn header_icon<'a, M: Clone + 'a>(
    name: &'static str,
    checked: bool,
    on_press: M,
) -> button::Button<'a, M> {
    widget::button::custom(icon::from_name(name).size(16).icon())
        .padding(9)
        .class(flat_button(checked))
        .on_press(on_press)
}

/// A menu title (File, Edit…) as libadwaita's flat text button.
pub fn menu_root<'a, M: Clone + 'a>(label: String) -> Element<'a, M> {
    widget::button::custom(widget::text::body(label))
        .padding([7, 10])
        .class(shaded_with(6.0, None, 0.07, 0.16, false))
        .into()
}

fn control<'a, M: Clone + 'a>(name: &'static str, on_press: M) -> Element<'a, M> {
    widget::button::custom(
        widget::container(icon::from_name(name).size(16).icon())
            .center(Length::Fixed(24.0)),
    )
    .padding(0)
    .width(Length::Fixed(24.0))
    .height(Length::Fixed(24.0))
    .class(shaded(12.0, Some(0.10), 0.15, 0.30))
    .on_press(on_press)
    .into()
}

/// libadwaita's window controls, in GNOME's order (minimize, maximize,
/// close), each shown only when GNOME's `button-layout` asks for it.
pub fn window_controls<'a, M: Clone + 'a>(
    maximized: bool,
    minimize: Option<M>,
    maximize: Option<M>,
    close: M,
) -> Element<'a, M> {
    let mut row = widget::row::with_capacity(3)
        .spacing(12)
        .align_y(cosmic::iced::Alignment::Center)
        // Mutter's controls sit a little apart from the app's own buttons.
        .padding([0, 2, 0, 8]);
    if let Some(m) = minimize {
        row = row.push(control("window-minimize-symbolic", m));
    }
    if let Some(m) = maximize {
        let name = if maximized {
            "window-restore-symbolic"
        } else {
            "window-maximize-symbolic"
        };
        row = row.push(control(name, m));
    }
    row.push(control("window-close-symbolic", close)).into()
}

/// libadwaita's popover: its own colour, a 12 px radius, 6 px padding and
/// the faint outline GNOME draws round floating surfaces.
pub fn menu_style() -> theme::menu_bar::MenuBarStyle {
    fn appearance(theme: &cosmic::Theme) -> theme::menu_bar::Appearance {
        let dark = theme.cosmic().is_dark;
        let background = if dark {
            Color::from_rgb8(0x36, 0x36, 0x3a)
        } else {
            Color::WHITE
        };
        let border_color = if dark {
            Color::from_rgba(1.0, 1.0, 1.0, 0.10)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.12)
        };
        theme::menu_bar::Appearance {
            background,
            border_width: 1.0,
            bar_border_radius: [6.0; 4],
            menu_border_radius: [12.0; 4],
            border_color,
            background_expand: [6; 4],
            path: tint(theme, 0.10),
        }
    }
    theme::menu_bar::MenuBarStyle::from(appearance as fn(&cosmic::Theme) -> _)
}
