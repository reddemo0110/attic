// SPDX-License-Identifier: GPL-3.0-only

//! The rich editor widget: draws a `RichContent` buffer straight from
//! cosmic-text (`fill_raw`), with caret, selection, focus, mouse and
//! keyboard handling modelled on iced's `text_editor` (the fork at
//! libcosmic `c003a58`). Phase 1 renders exactly what the stock editor did;
//! phase 2 layers the rich attributes and overlays on top.

use super::content::{Highlight, HotKind, RichContent};
use crate::markdown;
use crate::retro::Palette;
use cosmic::iced::advanced::clipboard::{self, Clipboard};
use cosmic::iced::advanced::input_method::{self, InputMethod};
use cosmic::iced::advanced::mouse;
use cosmic::iced::advanced::renderer::{self, Quad};
use cosmic::iced::advanced::svg::Renderer as _;
use cosmic::iced::advanced::text::Renderer as _;
use cosmic::iced::advanced::widget::{self, Operation, Tree, operation, tree};
use cosmic::iced::advanced::{
    Layout, Renderer as _, Shell, Widget, graphics::text as gtext, layout,
};
use cosmic::iced::keyboard::{self, key};
use cosmic::iced::{
    Background, Border, Color, Element, Event, Font, Length, Padding, Point, Rectangle, Size,
    Vector, window,
};
use cosmic::widget::text_editor::{Action, Edit, Motion};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Something the user asked to follow with Ctrl+click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Note(String),
    Tag(String),
}

pub struct RichEditor<'a, Message> {
    content: &'a RichContent,
    id: Option<widget::Id>,
    placeholder: String,
    font: Font,
    size: f32,
    line_height: f32,
    padding: Padding,
    min_height: f32,
    settings: markdown::Settings,
    palette: Palette,
    on_action: Option<Box<dyn Fn(Action) -> Message + 'a>>,
    on_link: Option<Box<dyn Fn(Link) -> Message + 'a>>,
    /// Fired when a middle-drag finishes over a selection: the highlighter.
    on_mark: Option<Box<dyn Fn() -> Message + 'a>>,
    /// Fired with the caret line's rectangle (layout coordinates) after
    /// the app asked for the caret to be revealed.
    on_caret: Option<Box<dyn Fn(Rectangle) -> Message + 'a>>,
    /// Label on the drop indicator ("picture drops here").
    drop_label: String,
    /// Trip time (seconds) and landing exponent for the drop marker's
    /// glide (`None` = snap).
    drop_anim: Option<(f32, f32)>,
}

impl<'a, Message> RichEditor<'a, Message> {
    pub fn new(content: &'a RichContent, settings: markdown::Settings) -> Self {
        Self {
            content,
            id: None,
            placeholder: String::new(),
            font: settings.font,
            size: 15.0,
            line_height: 22.5,
            padding: Padding::from([6, 10]),
            min_height: 0.0,
            palette: settings.palette,
            settings,
            on_action: None,
            on_link: None,
            on_mark: None,
            on_caret: None,
            drop_label: String::new(),
            drop_anim: None,
        }
    }

    pub fn drop_anim(mut self, anim: Option<(f32, f32)>) -> Self {
        self.drop_anim = anim;
        self
    }

    pub fn drop_label(mut self, label: String) -> Self {
        self.drop_label = label;
        self
    }

    pub fn on_link(mut self, f: impl Fn(Link) -> Message + 'a) -> Self {
        self.on_link = Some(Box::new(f));
        self
    }

    pub fn on_caret(mut self, f: impl Fn(Rectangle) -> Message + 'a) -> Self {
        self.on_caret = Some(Box::new(f));
        self
    }

    pub fn on_mark(mut self, f: impl Fn() -> Message + 'a) -> Self {
        self.on_mark = Some(Box::new(f));
        self
    }

    /// Where the input method should put its popup, plus the preedit.
    fn input_method<'b>(&self, state: &'b State, bounds: Rectangle) -> InputMethod<&'b str> {
        let Some(Focus {
            window_focused: true,
            ..
        }) = &state.focus
        else {
            return InputMethod::Disabled;
        };
        let origin = bounds.shrink(self.padding).position() - Point::ORIGIN;
        let caret = match self.content.highlight() {
            Highlight::Caret(p) => p,
            Highlight::Range(r) => r.first().map_or(Point::ORIGIN, |r| r.position()),
        };
        InputMethod::Enabled {
            cursor: Rectangle::new(
                caret + origin,
                Size::new(1.0, self.content.line_height_at_cursor()),
            ),
            purpose: input_method::Purpose::Normal,
            preedit: state.preedit.as_ref().map(input_method::Preedit::as_ref),
        }
    }

    /// The hotspot under `point` (widget-local), if any.
    fn hotspot_at(&self, point: Point) -> Option<HotKind> {
        let p = point - Vector::new(self.padding.left, self.padding.top);
        self.content
            .overlays()
            .hotspots
            .into_iter()
            .find(|h| h.rect.contains(p))
            .map(|h| h.kind)
    }

    pub fn id(mut self, id: widget::Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn placeholder(mut self, text: String) -> Self {
        self.placeholder = text;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self.line_height = size * 1.5;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn min_height(mut self, h: f32) -> Self {
        self.min_height = h;
        self
    }

    pub fn on_action(mut self, f: impl Fn(Action) -> Message + 'a) -> Self {
        self.on_action = Some(Box::new(f));
        self
    }
}

#[derive(Debug)]
pub struct State {
    focus: Option<Focus>,
    modifiers: keyboard::Modifiers,
    /// Text being composed by an input method, shown at the caret.
    preedit: Option<input_method::Preedit>,
    /// The line showing its raw markdown (double-clicked or being edited).
    /// Cleared as soon as the caret leaves it.
    revealed: Option<usize>,
    /// Reveal the caret's line at the next layout.
    reveal_pending: bool,
    last_click: Option<mouse::Click>,
    drag_click: Option<mouse::click::Kind>,
    /// A middle-button drag in flight: on release it becomes a `==mark==`.
    mark_drag: bool,
    /// Emitted on the next event once focus was lost by a click elsewhere.
    pending: Option<Action>,
    /// The drop marker's glide (buffer coordinates) and its last frame.
    marker: crate::anim::Glide,
    marker_frame: Option<Instant>,
}

#[derive(Debug, Clone)]
struct Focus {
    updated_at: Instant,
    now: Instant,
    window_focused: bool,
}

impl Focus {
    const BLINK_MS: u128 = 500;

    fn now() -> Self {
        let now = Instant::now();
        Self {
            updated_at: now,
            now,
            window_focused: true,
        }
    }

    fn caret_visible(&self) -> bool {
        self.window_focused
            && ((self.now - self.updated_at).as_millis() / Self::BLINK_MS).is_multiple_of(2)
    }
}

impl State {
    fn clear_focus(&mut self) {
        self.focus = None;
        self.drag_click = None;
        self.pending = Some(Action::ClearSelection);
    }
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.focus.is_some()
    }
    fn focus(&mut self) {
        self.focus = Some(Focus::now());
    }
    fn unfocus(&mut self) {
        self.clear_focus();
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for RichEditor<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            focus: None,
            modifiers: keyboard::Modifiers::default(),
            preedit: None,
            revealed: None,
            reveal_pending: false,
            last_click: None,
            drag_click: None,
            mark_drag: false,
            pending: None,
            marker: crate::anim::Glide::default(),
            marker_frame: None,
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let inner_w = (width - self.padding.left - self.padding.right).max(1.0);
        // A line shows its raw markdown only while revealed (double-click
        // or an edit) and the caret is still on it; a single click keeps
        // the rendered look.
        let state = tree.state.downcast_mut::<State>();
        let caret_line = self.content.cursor().position.line;
        if self.content.take_render_hint() {
            state.reveal_pending = false;
            state.revealed = None;
        }
        if state.reveal_pending {
            state.reveal_pending = false;
            state.revealed = Some(caret_line);
        }
        if state.revealed.is_some_and(|l| l != caret_line) || state.focus.is_none() {
            state.revealed = None;
        }
        let active = state.revealed;
        self.content.update(
            inner_w,
            self.font,
            self.size,
            self.line_height,
            &self.settings,
            active,
        );
        let h =
            (self.content.height() + self.padding.top + self.padding.bottom).max(self.min_height);
        layout::Node::new(Size::new(width, h))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        // Keep the app's picture of where this editor sits current even when
        // it is scrolled out of view and never drawn.
        self.content.set_bounds(bounds);
        // The app asked this editor to take keyboard focus.
        if self.content.take_focus_request() {
            state.focus = Some(Focus::now());
            shell.request_redraw();
        }
        // …or to let it go (a sibling editor or a table has the keys).
        if self.content.take_unfocus_request() && state.focus.is_some() {
            state.focus = None;
            state.drag_click = None;
            shell.request_redraw();
        }
        // The app edited or moved the caret; now that the buffer is laid
        // out, tell it where the caret's line is so the note can scroll.
        if let Some(on_caret) = &self.on_caret
            && self.content.take_reveal_request()
        {
            let (top, height) = self.content.caret_line_span();
            shell.publish(on_caret(Rectangle {
                x: bounds.x,
                y: bounds.y + self.padding.top + top,
                width: bounds.width,
                height,
            }));
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                self.content.set_pointer_y(cursor.position().map(|p| p.y));
            }
            Event::Mouse(mouse::Event::CursorLeft) => self.content.set_pointer_y(None),
            _ => {}
        }

        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && cursor.position_over(bounds).is_none()
            && state.focus.is_some()
        {
            state.clear_focus();
        }
        if let Some(action) = state.pending.take()
            && let Some(on) = &self.on_action
        {
            shell.publish(on(action));
        }
        let Some(on_action) = self.on_action.as_ref() else {
            return;
        };

        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => state.modifiers = *m,
            Event::Window(window::Event::Unfocused) => {
                if let Some(f) = &mut state.focus {
                    f.window_focused = false;
                }
            }
            Event::Window(window::Event::Focused) => {
                if let Some(f) = &mut state.focus {
                    f.window_focused = true;
                    f.updated_at = Instant::now();
                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                // The drop marker glides toward its line rather than jumping.
                if let Some(line) = self.content.drop_marker() {
                    let dt = state
                        .marker_frame
                        .replace(*now)
                        .map_or(0.016, |t| now.duration_since(t).as_secs_f32());
                    state.marker.to(self.content.line_top(line), self.drop_anim);
                    if state.marker.step(dt, self.drop_anim) {
                        shell.request_redraw();
                    }
                } else {
                    state.marker.clear();
                    state.marker_frame = None;
                }
                if let Some(f) = &mut state.focus
                    && f.window_focused
                {
                    f.now = *now;
                    let until =
                        Focus::BLINK_MS - (f.now - f.updated_at).as_millis() % Focus::BLINK_MS;
                    shell.request_redraw_at(f.now + Duration::from_millis(until as u64));
                }
                shell.request_input_method(&self.input_method(state, bounds));
            }
            Event::InputMethod(ime) => match ime {
                input_method::Event::Opened | input_method::Event::Closed => {
                    state.preedit =
                        matches!(ime, input_method::Event::Opened).then(input_method::Preedit::new);
                    shell.request_redraw();
                }
                input_method::Event::Preedit(content, selection) if state.focus.is_some() => {
                    state.preedit = Some(input_method::Preedit {
                        content: content.clone(),
                        selection: selection.clone(),
                        text_size: Some(self.size.into()),
                    });
                    shell.request_redraw();
                }
                input_method::Event::Commit(text) if state.focus.is_some() => {
                    shell.publish(on_action(Action::Edit(Edit::Paste(Arc::new(text.clone())))));
                }
                _ => {}
            },
            _ => {}
        }

        let text_origin = Vector::new(self.padding.left, self.padding.top);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds) {
                    // Ctrl+click on a link or tag follows it instead of placing the caret.
                    if state.modifiers.command()
                        && let Some(on_link) = &self.on_link
                        && let Some(kind) = self.hotspot_at(p)
                    {
                        let link = match kind {
                            HotKind::Link(t) => Some(Link::Note(t)),
                            HotKind::Tag(t) => Some(Link::Tag(t)),
                            HotKind::Task => None,
                        };
                        if let Some(link) = link {
                            shell.publish(on_link(link));
                            shell.capture_event();
                            return;
                        }
                    }
                    let p = p - text_origin;
                    let click = mouse::Click::new(p, mouse::Button::Left, state.last_click);
                    let action = match click.kind() {
                        mouse::click::Kind::Single => Action::Click(p),
                        // Double-click reveals the line's markdown for editing.
                        mouse::click::Kind::Double => {
                            state.reveal_pending = true;
                            Action::Click(p)
                        }
                        mouse::click::Kind::Triple => Action::SelectLine,
                    };
                    state.focus = Some(Focus::now());
                    state.last_click = Some(click);
                    state.drag_click = Some(click.kind());
                    shell.publish(on_action(action));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_click = None;
            }
            // Middle-drag is the highlighter: sweep over text like a marker
            // pen, and on release the selection becomes `==marked==`.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                if self.on_mark.is_some()
                    && let Some(p) = cursor.position_in(bounds)
                {
                    let p = p - text_origin;
                    state.focus = Some(Focus::now());
                    state.mark_drag = true;
                    state.drag_click = Some(mouse::click::Kind::Single);
                    shell.publish(on_action(Action::Click(p)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Middle)) => {
                if state.mark_drag {
                    state.mark_drag = false;
                    state.drag_click = None;
                    if self.content.selection().is_some()
                        && let Some(on_mark) = &self.on_mark
                    {
                        shell.publish(on_mark());
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.drag_click == Some(mouse::click::Kind::Single)
                    && let Some(p) = cursor.position_in(bounds)
                {
                    shell.publish(on_action(Action::Drag(p - text_origin)));
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                modifiers,
                text,
                ..
            }) if state.focus.is_some() => {
                let bindings = key_bindings(
                    key,
                    modified_key,
                    *physical_key,
                    *modifiers,
                    text.as_deref(),
                );
                let Some(bindings) = bindings else { return };
                let mut captured = true;
                for b in bindings {
                    match b {
                        Binding::Unfocus => {
                            state.focus = None;
                            state.drag_click = None;
                            captured = false;
                        }
                        Binding::Copy => {
                            if let Some(s) = self.content.selection() {
                                clipboard.write(clipboard::Kind::Standard, s);
                            }
                        }
                        Binding::Cut => {
                            if let Some(s) = self.content.selection() {
                                clipboard.write(clipboard::Kind::Standard, s);
                                shell.publish(on_action(Action::Edit(Edit::Delete)));
                            }
                        }
                        Binding::Paste => {
                            if let Some(s) = clipboard.read(clipboard::Kind::Standard) {
                                shell.publish(on_action(Action::Edit(Edit::Paste(Arc::new(s)))));
                            }
                        }
                        Binding::Action(a) => {
                            // Typing a marker, deleting, or pasting on a rendered
                            // line shows its markdown; plain text keeps it rendered.
                            let reveals = match &a {
                                Action::Edit(Edit::Insert(c)) => {
                                    matches!(c, '*' | '_' | '`' | '~' | '[' | ']' | '>' | '#')
                                }
                                // Deleting plain text keeps the line rendered; only
                                // eating into a hidden marker shows the markdown.
                                Action::Edit(Edit::Backspace) => {
                                    self.content.delete_touches_marker(true)
                                }
                                Action::Edit(Edit::Delete) => {
                                    self.content.delete_touches_marker(false)
                                }
                                _ => false,
                            };
                            if reveals {
                                state.reveal_pending = true;
                            }
                            shell.publish(on_action(a));
                        }
                    }
                }
                if captured {
                    shell.capture_event();
                }
                if let Some(f) = &mut state.focus {
                    f.updated_at = Instant::now();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        _theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let text_bounds = bounds.shrink(self.padding);
        let origin = text_bounds.position() - Point::ORIGIN;
        let p = &self.palette;

        let highlight = self.content.highlight();
        let overlays = self.content.overlays();
        let at = |r: &Rectangle| Rectangle {
            x: r.x + origin.x,
            y: r.y + origin.y,
            ..*r
        };
        // Backgrounds first: code blocks, code spans.
        for r in &overlays.code_block_rows {
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    ..Quad::default()
                },
                Background::Color(p.accent2.scale_alpha(0.08)),
            );
        }
        for r in &overlays.code_bgs {
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Quad::default()
                },
                Background::Color(p.accent2.scale_alpha(0.15)),
            );
        }
        // Highlighter bands: a marker stroke behind `==marked==` text.
        for r in &overlays.marks {
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Quad::default()
                },
                Background::Color(p.accent.scale_alpha(0.32)),
            );
        }
        for r in &overlays.quote_bars {
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    border: Border {
                        radius: 1.5.into(),
                        ..Default::default()
                    },
                    ..Quad::default()
                },
                Background::Color(p.dim),
            );
        }
        if let (Some(_), Highlight::Range(ranges)) = (&state.focus, &highlight) {
            for r in ranges {
                if let Some(r) = text_bounds.intersection(&(*r + origin)) {
                    renderer.fill_quad(
                        Quad {
                            bounds: r,
                            ..Quad::default()
                        },
                        Background::Color(p.sel),
                    );
                }
            }
        }

        if self.content.is_empty() && !self.placeholder.is_empty() {
            renderer.fill_text(
                cosmic::iced::advanced::Text {
                    content: self.placeholder.clone(),
                    bounds: text_bounds.size(),
                    size: self.size.into(),
                    line_height: cosmic::iced::widget::text::LineHeight::Absolute(
                        self.line_height.into(),
                    ),
                    font: self.font,
                    align_x: cosmic::iced::advanced::text::Alignment::Default,
                    align_y: cosmic::iced::alignment::Vertical::Top,
                    shaping: cosmic::iced::advanced::text::Shaping::Advanced,
                    wrapping: cosmic::iced::advanced::text::Wrapping::Word,
                    ellipsize: cosmic::iced::advanced::text::Ellipsize::None,
                },
                text_bounds.position(),
                p.dim,
                text_bounds,
            );
        } else {
            renderer.fill_raw(gtext::Raw {
                buffer: self.content.weak_buffer(),
                position: text_bounds.position(),
                color: p.fg,
                clip_bounds: text_bounds,
            });
        }

        // Proofreading: red under a misspelling, blue under a grammar slip.
        for (r, kind) in &overlays.proof {
            let color = match kind {
                jjb_core::proof::Kind::Spelling => Color::from_rgb(0.90, 0.30, 0.30),
                jjb_core::proof::Kind::Grammar => Color::from_rgb(0.35, 0.55, 0.95),
            };
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    ..Quad::default()
                },
                Background::Color(color.scale_alpha(0.85)),
            );
        }

        // On top of the glyphs: strikes, task boxes with their mark, bullets.
        for (r, c) in &overlays.strikes {
            let color = c.map_or(p.fg.scale_alpha(0.6), |c| {
                let [r, g, b, a] = c.as_rgba();
                Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
            });
            renderer.fill_quad(
                Quad {
                    bounds: at(r),
                    ..Quad::default()
                },
                Background::Color(color),
            );
        }
        for b in &overlays.boxes {
            let rect = at(&b.rect);
            renderer.fill_quad(
                Quad {
                    bounds: rect,
                    border: Border {
                        color: if b.done { p.accent } else { p.dim },
                        width: 1.5,
                        radius: 3.0.into(),
                    },
                    ..Quad::default()
                },
                Background::Color(if b.done {
                    p.accent.scale_alpha(0.18)
                } else {
                    Color::TRANSPARENT
                }),
            );
            if !b.mark.is_empty() {
                renderer.fill_text(
                    cosmic::iced::advanced::Text {
                        content: b.mark.clone(),
                        bounds: Size::new(rect.width, rect.height),
                        size: (rect.height * 0.8).into(),
                        line_height: cosmic::iced::widget::text::LineHeight::Absolute(
                            rect.height.into(),
                        ),
                        font: self.font,
                        align_x: cosmic::iced::advanced::text::Alignment::Center,
                        align_y: cosmic::iced::alignment::Vertical::Center,
                        shaping: cosmic::iced::advanced::text::Shaping::Advanced,
                        wrapping: cosmic::iced::advanced::text::Wrapping::None,
                        ellipsize: cosmic::iced::advanced::text::Ellipsize::None,
                    },
                    Point::new(rect.center_x(), rect.center_y()),
                    p.accent,
                    rect.expand(4.0),
                );
            }
        }
        // Folder icons over the hash of tags that wear one.
        for (r, icon) in &overlays.tag_icons {
            let rect = at(r);
            // Centred on the hash; the small overhang lands in the side bearings.
            let side = self.size * 0.95;
            let bounds = Rectangle {
                x: rect.center_x() - side / 2.0,
                y: rect.center_y() - side / 2.0,
                width: side,
                height: side,
            };
            renderer.draw_svg(
                cosmic::iced::advanced::svg::Svg::new(
                    icon.handle(self.settings.icon_set, p.accent2),
                )
                .color(p.accent2),
                bounds,
                bounds.expand(2.0),
            );
        }
        for r in &overlays.bullets {
            let rect = at(r);
            renderer.fill_text(
                cosmic::iced::advanced::Text {
                    content: "•".to_owned(),
                    bounds: Size::new(rect.width, rect.height),
                    size: self.size.into(),
                    line_height: cosmic::iced::widget::text::LineHeight::Absolute(
                        rect.height.into(),
                    ),
                    font: self.font,
                    align_x: cosmic::iced::advanced::text::Alignment::Left,
                    align_y: cosmic::iced::alignment::Vertical::Center,
                    shaping: cosmic::iced::advanced::text::Shaping::Advanced,
                    wrapping: cosmic::iced::advanced::text::Wrapping::None,
                    ellipsize: cosmic::iced::advanced::text::Ellipsize::None,
                },
                Point::new(rect.x + 2.0, rect.center_y()),
                p.accent,
                rect.expand(2.0),
            );
        }

        // Ctrl held over a link or tag: underline it.
        if state.modifiers.command()
            && let Some(pos) = _cursor.position_in(bounds)
        {
            let local = pos - origin;
            if let Some(h) = overlays
                .hotspots
                .iter()
                .find(|h| h.rect.contains(local) && !matches!(h.kind, HotKind::Task))
            {
                let r = at(&h.rect);
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: r.x,
                            y: r.y + r.height - 3.0,
                            width: r.width,
                            height: 1.5,
                        },
                        ..Quad::default()
                    },
                    Background::Color(p.accent2),
                );
            }
        }

        // A file being dragged over the note: the line where it would land.
        if let Some(line) = self.content.drop_marker() {
            let y = text_bounds.y
                + if state.marker.shown() {
                    state.marker.pos
                } else {
                    self.content.line_top(line)
                }
                - 1.0;
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: text_bounds.x,
                        y,
                        width: text_bounds.width,
                        height: 2.0,
                    },
                    border: Border {
                        radius: 1.0.into(),
                        ..Default::default()
                    },
                    ..Quad::default()
                },
                Background::Color(p.accent),
            );
            if !self.drop_label.is_empty() {
                renderer.fill_text(
                    cosmic::iced::advanced::Text {
                        content: self.drop_label.clone(),
                        bounds: Size::new(text_bounds.width, 16.0),
                        size: 12.0.into(),
                        line_height: cosmic::iced::widget::text::LineHeight::Absolute(16.0.into()),
                        font: self.font,
                        align_x: cosmic::iced::advanced::text::Alignment::Center,
                        align_y: cosmic::iced::alignment::Vertical::Center,
                        shaping: cosmic::iced::advanced::text::Shaping::Advanced,
                        wrapping: cosmic::iced::advanced::text::Wrapping::None,
                        ellipsize: cosmic::iced::advanced::text::Ellipsize::None,
                    },
                    Point::new(text_bounds.center_x(), y + 1.0),
                    p.accent,
                    Rectangle {
                        x: text_bounds.x,
                        y: y - 8.0,
                        width: text_bounds.width,
                        height: 18.0,
                    },
                );
            }
        }

        if let (Some(f), Highlight::Caret(at)) = (&state.focus, &highlight)
            && f.caret_visible()
        {
            let caret = Rectangle::new(
                *at + origin,
                Size::new(1.5, self.content.line_height_at_cursor()),
            );
            if let Some(c) = text_bounds.intersection(&caret) {
                renderer.fill_quad(
                    Quad {
                        bounds: c,
                        border: Border {
                            radius: 1.0.into(),
                            ..Default::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(p.caret),
                );
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let Some(p) = cursor.position_in(layout.bounds()) else {
            return mouse::Interaction::default();
        };
        if self.on_action.is_none() {
            return mouse::Interaction::NotAllowed;
        }
        let state = tree.state.downcast_ref::<State>();
        match self.hotspot_at(p) {
            Some(HotKind::Task) => mouse::Interaction::Pointer,
            Some(_) if state.modifiers.command() => mouse::Interaction::Pointer,
            _ => mouse::Interaction::Text,
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &cosmic::Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(self.id.as_ref(), layout.bounds(), state);
    }

    fn id(&self) -> Option<widget::Id> {
        self.id.clone()
    }

    fn set_id(&mut self, id: widget::Id) {
        self.id = Some(id);
    }
}

impl<'a, Message: 'a> From<RichEditor<'a, Message>>
    for Element<'a, Message, cosmic::Theme, cosmic::Renderer>
{
    fn from(e: RichEditor<'a, Message>) -> Self {
        Element::new(e)
    }
}

enum Binding {
    Unfocus,
    Copy,
    Cut,
    Paste,
    Action(Action),
}

/// Keyboard → editor actions, the same table iced's editor uses.
fn key_bindings(
    key: &keyboard::Key,
    modified_key: &keyboard::Key,
    physical: key::Physical,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> Option<Vec<Binding>> {
    let one = |b| Some(vec![b]);
    match key.to_latin(physical) {
        Some('c') if modifiers.command() => return one(Binding::Copy),
        Some('x') if modifiers.command() => return one(Binding::Cut),
        Some('v') if modifiers.command() && !modifiers.alt() => return one(Binding::Paste),
        Some('a') if modifiers.command() => return one(Binding::Action(Action::SelectAll)),
        _ => {}
    }
    if let keyboard::Key::Named(named) = key.as_ref() {
        match named {
            key::Named::Insert if modifiers.shift() => return one(Binding::Paste),
            key::Named::Insert if modifiers.command() => return one(Binding::Copy),
            key::Named::Delete if modifiers.shift() => return one(Binding::Cut),
            _ => {}
        }
    }
    // Ctrl/Alt + a letter belongs to the app's own shortcuts (Ctrl+B …).
    if (modifiers.command() || modifiers.alt())
        && matches!(key.as_ref(), keyboard::Key::Character(_))
    {
        return None;
    }
    match modified_key.as_ref() {
        keyboard::Key::Named(key::Named::Enter) => one(Binding::Action(Action::Edit(Edit::Enter))),
        keyboard::Key::Named(key::Named::Backspace) => {
            one(Binding::Action(Action::Edit(Edit::Backspace)))
        }
        keyboard::Key::Named(key::Named::Delete) if text.is_none() || text == Some("\u{7f}") => {
            one(Binding::Action(Action::Edit(Edit::Delete)))
        }
        keyboard::Key::Named(key::Named::Escape) => one(Binding::Unfocus),
        keyboard::Key::Named(key::Named::Tab) => {
            one(Binding::Action(Action::Edit(if modifiers.shift() {
                Edit::Unindent
            } else {
                Edit::Indent
            })))
        }
        _ => {
            if let Some(text) = text {
                let c = text.chars().find(|c| !c.is_control())?;
                one(Binding::Action(Action::Edit(Edit::Insert(c))))
            } else if let keyboard::Key::Named(named) = key.as_ref() {
                let motion = match named {
                    key::Named::ArrowLeft => Motion::Left,
                    key::Named::ArrowRight => Motion::Right,
                    key::Named::ArrowUp => Motion::Up,
                    key::Named::ArrowDown => Motion::Down,
                    key::Named::Home => Motion::Home,
                    key::Named::End => Motion::End,
                    key::Named::PageUp => Motion::PageUp,
                    key::Named::PageDown => Motion::PageDown,
                    _ => return None,
                };
                let motion = if modifiers.jump() {
                    motion.widen()
                } else {
                    motion
                };
                one(Binding::Action(if modifiers.shift() {
                    Action::Select(motion)
                } else {
                    Action::Move(motion)
                }))
            } else {
                None
            }
        }
    }
}
