// SPDX-License-Identifier: GPL-3.0-only

use crate::blocks::{Block, Blocks};
use crate::config::Config;
use crate::debug_script::{self, Step};
use crate::dockicon::DockIcon;
use crate::fl;
use crate::images::{self, Align, FrameStyle, ImageRef, PickerEntry, Processed, UriList};
use crate::markdown;
use crate::note::{self, Note, NoteSummary};
use crate::retro::{self, Palette};
use crate::store::{Applied, Store, View};
use crate::sync::{self, Session};
use chrono::{DateTime, Datelike, Local, Utc};
use cosmic::Application as _;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::keyboard::{self, key::Physical};
use cosmic::iced::{
    Alignment, Event, Length, Point, Rectangle, Subscription, Vector, event, mouse, window,
};
use cosmic::prelude::*;
use cosmic::widget::menu::action::MenuAction as _;
use cosmic::widget::{self, about::About, icon, menu, nav_bar, text_editor};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");
/// How long after the last keystroke we write the note to disk.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(600);
/// Sync this long after the last local change.
const SYNC_AFTER_SAVE: Duration = Duration::from_secs(3);
/// And at least this often while signed in, to pick up other devices.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Undo steps kept per open note.
const UNDO_DEPTH: usize = 200;
/// Typing separated by less than this is one undo step.
const UNDO_GROUP_IDLE: Duration = Duration::from_millis(700);
const NAV_WIDTH: f32 = 244.0;
const NOTE_LIST_WIDTH: f32 = 340.0;
/// Pointer travel (px) that turns a press on a picture into a drag.
const DRAG_THRESHOLD: f32 = 6.0;

pub struct AppModel {
    core: cosmic::Core,
    context_page: ContextPage,
    about: About,
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    config: Config,
    config_handler: Option<cosmic_config::Config>,
    theme: retro::Theme,
    /// The colour buffet: a highlight (a minimal theme) on a dark plate,
    /// or on a paper plate with a chosen ink, painted instead of `theme`
    /// while `buffet_on`.
    buffet_on: bool,
    buffet_light_mode: bool,
    buffet_highlight: retro::Theme,
    buffet_dark: retro::Dark,
    buffet_light: retro::Light,
    buffet_ink: retro::Ink,
    show_markers: bool,
    show_nav: bool,
    show_list: bool,
    /// Editor typeface, per-pane text sizes (px) and dock scale — all persisted.
    editor_font: retro::EditorFont,
    /// Editor body weight (200 | 300 | 400 | 500).
    editor_weight: u16,
    /// Sidebar + list face, and the pane-title face (a designer pairing sets all three).
    ui_font: retro::EditorFont,
    /// The notes list's own face (follows `ui_font` until set apart).
    list_font: retro::EditorFont,
    title_font: retro::EditorFont,
    /// Which pane the font picker's clicks and size stepper act on.
    font_target: Pane,
    font_size: u16,
    sidebar_size: u16,
    list_size: u16,
    dock_size: retro::DockSize,
    /// What a finished task shows inside its brackets.
    task_marker: String,
    /// Maximum width of the note text column.
    measure: retro::Measure,
    /// Launcher icon: a theme's colours, or `None` to follow the theme.
    icon_theme: Option<retro::Theme>,
    /// Which sections of the Appearance drawer are unfolded.
    appearance_open: [bool; 9],
    /// Cloud sync: the account, the form, and the cycle in flight.
    sync: SyncUi,
    /// Processed images keyed by path|mtime|style|theme.
    image_cache: HashMap<String, ImageState>,
    /// Block index of the image whose ⋯ menu is open.
    image_menu: Option<usize>,
    /// Where `[]` just became `- [ ] `: (block, line, column). The space
    /// a writer types next is the one the expansion already added.
    task_space: Option<(usize, usize, usize)>,
    /// Drag-resize in progress.
    resizing: Option<Resize>,
    /// Table cell being edited (block, row, col) and its raw draft text.
    table_edit: Option<(usize, usize, usize)>,
    table_draft: String,
    /// Whether the open cell's draft was typed into: fresh cells let ←/→
    /// roam the grid; a touched draft keeps them for the text caret.
    table_touched: bool,
    table_input: widget::Id,
    /// A live column/row resize on a table.
    table_resize: Option<TableResize>,
    /// Excel-style pointing: while a formula draft expects a reference,
    /// clicking or sweeping cells writes the ref/range into the draft.
    table_pick: Option<TablePick>,
    /// The fill handle mid-drag: replicate the source cell down or across
    /// on release, formulas translated relative to each target row/column.
    table_fill: Option<TableFill>,
    /// Cells being rubber-band selected: press-drag across the grid, then
    /// Delete/Backspace clears them. A press that never leaves its cell
    /// becomes a normal click-to-edit on release.
    table_sel: Option<TableSel>,
    /// Weight of drawn lines: the divider, and the card border (with its
    /// own on/off).
    rule_size: retro::LineSize,
    card_line: retro::LineSize,
    tag_line_size: retro::LineSize,
    card_border: bool,
    /// Live window size, persisted (debounced by the autosave tick).
    win_size: Option<(u32, u32)>,
    /// The Ctrl+Shift+H shortcuts overlay.
    show_shortcuts: bool,
    /// The neon coffee sign (Ctrl+Shift+Enter): current steam frame and glow.
    coffee: Option<(usize, f32)>,
    coffee_seed: u32,
    /// The hidden theme has been found.
    coffee_unlocked: bool,
    /// A picture being dragged to another line (press, then move).
    dragging: Option<ImageDrag>,
    /// Scroll pane under the pointer: only it shows its scrollbar.
    scroll_hover: Option<ScrollArea>,
    /// The pointer left a scroll pane mid-drag; hide its bar on release.
    scroll_hover_release: bool,
    /// Left mouse button is down (tracked for the scrollbar hand-off above).
    mouse_down: bool,
    /// A sidebar tag (with its subtags) or spacer being dragged to a new spot.
    tag_drag: Option<TagDrag>,
    /// The add-a-spacer strip at the bottom of the tags pane is hovered.
    space_hint: bool,
    /// Glide time of moving UI in ms (drop indicators and friends); 0 = off.
    anim_ms: u16,
    /// Landing softness: ease-out exponent in tenths (10 = linear).
    anim_ease10: u16,
    /// Gliding y of the tags-pane drop indicator, within the tag list.
    tag_line: crate::anim::Glide,
    /// Bounds of each root entry's rows, and of the whole tag list, as
    /// recorded by probes in the view.
    tag_rows: std::cell::RefCell<Vec<std::rc::Rc<std::cell::Cell<cosmic::iced::Rectangle>>>>,
    tag_area: std::rc::Rc<std::cell::Cell<cosmic::iced::Rectangle>>,
    /// The same for the note body during an image/card drag.
    note_line: crate::anim::Glide,
    note_slots: std::cell::RefCell<
        std::collections::HashMap<usize, std::rc::Rc<std::cell::Cell<cosmic::iced::Rectangle>>>,
    >,
    note_area: std::rc::Rc<std::cell::Cell<cosmic::iced::Rectangle>>,
    /// The previous animation frame, for frame-time deltas.
    anim_last: Option<Instant>,
    /// Last known cursor position (window coords), for drag deltas.
    mouse_x: f32,
    mouse_y: f32,
    /// Width being dragged, shown live before it is written to the note.
    live_width: Option<(usize, u32)>,
    /// In-app image picker (context drawer).
    picker_dir: PathBuf,
    picker_entries: Vec<PickerEntry>,
    /// Thumbnails for the picker's grid, by file path.
    thumbs: HashMap<PathBuf, ImageState>,
    /// Grid (thumbnails) or list.
    picker_grid: bool,
    /// The picker is attaching any file, not just pictures.
    picker_files: bool,
    /// Web link previews by address (fetched, cached or failed).
    previews: HashMap<String, PreviewState>,
    /// Which link card has its ⋯ menu open.
    link_menu: Option<usize>,
    /// Fetch title/description/picture for web links.
    link_previews: bool,
    /// A file drag is hovering the editor, and the body line it would land before.
    drop_hover: bool,
    drop_target: Option<usize>,

    store: Option<Store>,
    store_error: Option<String>,
    view: View,
    /// Note counts for the fixed views: all, untagged, trash.
    view_counts: [usize; 3],
    /// Tag tree (full path, count), depth-first.
    tags: Vec<(String, usize)>,
    /// Tags whose sub-tags are folded away.
    collapsed: HashSet<String>,
    /// Tag whose right-click menu is open.
    tag_menu: Option<String>,
    /// Tag being renamed and the draft name.
    tag_rename: Option<(String, String)>,
    /// Tag whose icon grid is open.
    tag_icon_pick: Option<String>,
    /// Folder icons by tag (shared with the editor's settings), and their style.
    tag_icons: Arc<HashMap<String, crate::glyph::Icon>>,
    icon_set: crate::glyph::IconSet,
    rename_id: widget::Id,
    query: String,
    search_id: widget::Id,
    notes_scroll_id: widget::Id,
    editor_scroll_id: widget::Id,
    /// The dock's `+` is expanded, showing new-note / new-folder.
    dock_open: bool,
    new_folder: String,
    folder_id: widget::Id,
    notes: Vec<NoteSummary>,
    current: Option<Note>,
    /// The open note's body as text/image blocks.
    blocks: Blocks,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Set when the last autosave failed; cleared by the next success.
    save_error: Option<String>,
    /// Show "saved hh:mm" next to the tick until this instant.
    saved_info_until: Option<Instant>,
    last_undo_kind: EditKind,
    last_undo_at: Instant,
    dirty: bool,
    last_edit: Instant,
    backlinks: Vec<NoteSummary>,
    /// Debug hook: `JJB_SCREENSHOT=/path.png` saves a frame ~2.5s after launch, then exits.
    /// Note: iced's screenshot path drops text-editor contents and menu labels; for a
    /// faithful capture use `tools/xshot.py` (see CLAUDE.md).
    screenshot_pending: bool,
    /// Debug hook: `JJB_SCRIPT=...` drives the app; see `debug_script.rs`.
    script: Option<debug_script::Runner>,
}

/// What the command line asked for. A second launch hands these to the
/// running instance over D-Bus (libcosmic's single-instance activation)
/// instead of opening another window.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    /// Files to open: notes in the folder open in place, anything else is
    /// imported as a copy. Absolute paths or `file://` URIs.
    pub open: Vec<String>,
    /// Text to put in the search box (GNOME's "search in app" arrow).
    pub search: Option<String>,
    /// Start on a fresh note (the launcher's "New note" action).
    pub new_note: bool,
}

/// The activation actions a second launch can forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchAction {
    Open,
    Search,
    NewNote,
}

impl std::fmt::Display for LaunchAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LaunchAction::Open => "open",
            LaunchAction::Search => "search",
            LaunchAction::NewNote => "new-note",
        })
    }
}

impl Flags {
    /// Parse the process arguments. Returns `None` for a mode that is not
    /// the GUI at all (`--search-provider`).
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<Flags> {
        let mut flags = Flags::default();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            match a.as_str() {
                "--search-provider" | "--launcher-plugin" => return None,
                "--new" | "--new-note" => flags.new_note = true,
                "--search" => flags.search = Some(args.by_ref().collect::<Vec<_>>().join(" ")),
                "--" => flags.open.extend(args.by_ref()),
                _ if a.starts_with("--") => tracing::warn!(arg = a, "unknown option ignored"),
                _ => flags.open.push(a),
            }
        }
        Some(flags)
    }

    fn action(&self) -> Option<LaunchAction> {
        if !self.open.is_empty() {
            Some(LaunchAction::Open)
        } else if self.search.is_some() {
            Some(LaunchAction::Search)
        } else if self.new_note {
            Some(LaunchAction::NewNote)
        } else {
            None
        }
    }

    /// The message that carries out what was asked, once the store is up.
    fn message(&self) -> Option<Message> {
        match self.action()? {
            LaunchAction::Open => Some(Message::OpenPaths(self.open.clone())),
            LaunchAction::Search => Some(Message::SearchFromOutside(
                self.search.clone().unwrap_or_default(),
            )),
            LaunchAction::NewNote => Some(Message::NewNote),
        }
    }
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = LaunchAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&LaunchAction> {
        // The running instance gets the action name plus args; a plain
        // launch just raises it.
        static OPEN: LaunchAction = LaunchAction::Open;
        static SEARCH: LaunchAction = LaunchAction::Search;
        static NEW: LaunchAction = LaunchAction::NewNote;
        match Flags::action(self)? {
            LaunchAction::Open => Some(&OPEN),
            LaunchAction::Search => Some(&SEARCH),
            LaunchAction::NewNote => Some(&NEW),
        }
    }

    fn args(&self) -> Vec<&str> {
        match Flags::action(self) {
            Some(LaunchAction::Open) => self.open.iter().map(String::as_str).collect(),
            Some(LaunchAction::Search) => self.search.iter().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    /// Open (or import) files named on the command line, dropped on the
    /// launcher, or picked from the GNOME Shell overview search.
    OpenPaths(Vec<String>),
    /// Fill the search box from outside (the shell's "search in app").
    SearchFromOutside(String),
    /// The desktop changed its fonts, icon theme or title-bar buttons
    /// while we run (GNOME, through the portal).
    DesktopSettings(crate::desktop::Settings),
    /// libcosmic installed a fresh system theme; off COSMIC, soften its
    /// window outline to the desktop's (see `desktop::softened_theme`).
    SoftenOutline,
    /// Something outside asked for the window: raise it.
    Raise,
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    Key(keyboard::Modifiers, keyboard::Key, Physical),
    /// ↑ / ↓ outside the editor: open the note above / below, like mail.
    NavigateNotes(i32),
    SetTheme(retro::Theme),
    /// Colour buffet: pick the highlight (a minimal theme), a dark plate,
    /// a paper plate, or the light side's ink. Any turns the buffet on
    /// (plates pick their side too); `SetTheme` turns it off.
    SetBuffetHighlight(retro::Theme),
    SetBuffetDark(retro::Dark),
    SetBuffetLight(retro::Light),
    SetBuffetInk(retro::Ink),
    SetFont(retro::EditorFont),
    /// Open a table cell for editing / edit the draft / commit and move.
    TableEdit(usize, usize, usize),
    TableDraft(String),
    TableCommit(TableMove),
    TableAddRow(usize),
    TableAddCol(usize),
    TableRemoveRow(usize),
    TableRemoveCol(usize),
    /// Start dragging a column (true) or row (false) edge: block, index,
    /// and the size the drag starts from.
    TableResizeStart(usize, bool, usize, f32),
    /// Formula pointing: press picks a cell ref, sweeping extends a range.
    TablePick(usize, usize, usize),
    TablePickOver(usize, usize, usize),
    /// Cycle the open cell's format: plain → $ → € → £ → ¥ → plain.
    TableMoney,
    /// The fill handle: press on the open cell's corner dot, sweep, release.
    TableFillStart(usize, usize, usize),
    TableFillOver(usize, usize, usize),
    /// Rubber-band cell selection: press, sweep, then Delete clears.
    TableSelStart(usize, usize, usize),
    TableSelOver(usize, usize, usize),
    /// Options: show or hide the table toolbar.
    SetTableToolbar(bool),
    /// The window was resized; remembered so next launch matches.
    WindowResized(f32, f32),
    /// Ticks only while the remembered size is unwritten; writes it.
    WindowSizeTick,
    /// Size options for drawn lines: the divider, the card border, and
    /// whether cards get a border at all.
    SetRuleSize(retro::LineSize),
    SetCardLine(retro::LineSize),
    SetTagLineSize(retro::LineSize),
    SetCardBorder(bool),
    /// Apply a designer pairing (titles, chrome, note) by key.
    SetPairing(String),
    /// Ctrl+click on a `[[link]]` or `#tag` in the rich editor.
    FollowLink(crate::editor::widget::Link),
    /// The focused editor reports its caret line (layout coordinates) so
    /// the note can scroll to keep it in view.
    RevealCaret(Rectangle),
    RestoreFonts,
    /// Grow (+) or shrink (−) one pane's text, in px.
    SizeStep(Pane, i16),
    SizeReset(Pane),
    ToggleSection(Section),
    SetDockSize(retro::DockSize),
    SetTaskMarker(String),
    SetMeasure(retro::Measure),
    /// Launcher icon: `Some(theme)` for that theme's colours, `None` to follow.
    SetIcon(Option<retro::Theme>),
    /// Fold or unfold the sub-tags of a tag in the sidebar.
    ToggleTagFold(String),
    /// Right-click menu on a tag (None closes it).
    TagMenu(Option<String>),
    TagRenameStart(String),
    TagRenameInput(String),
    TagRenameCommit,
    TagRenameCancel,
    /// Open the icon grid for a tag.
    TagIconPick(String),
    /// Give a tag an icon (`None` = back to `#`).
    SetTagIcon(String, Option<crate::glyph::Icon>),
    /// Folder icon style.
    SetIconSet(crate::glyph::IconSet),
    ToggleMarkers,
    ToggleNav,
    ToggleList,
    ToggleSolo,
    ImagesDropped(Vec<PathBuf>),
    PickImage,
    PickFile,
    PreviewLoaded(String, Result<crate::links::Preview, String>),
    LinkMenu(Option<usize>),
    RemoveLink(usize),
    CopyLink(String),
    RefreshPreview(String),
    OpenFile(String),
    SetLinkPreviews(bool),
    SyncUrl(String),
    SyncEmail(String),
    SyncPassword(String),
    SyncShowPassword,
    SyncSignIn,
    SyncSignUp,
    /// Sign-in, sign-up or a restored token came back.
    SyncAuth(Result<Session, String>),
    SyncSignOut,
    SyncNow,
    SyncTick,
    /// The keyring gave up its token (or had none).
    SyncTokenLoaded(Option<String>),
    SyncDone(Arc<sync::Outcome>),
    PickerNavigate(PathBuf),
    PickerChoose(PathBuf),
    /// A picker thumbnail finished decoding.
    ThumbLoaded(PathBuf, Result<(u32, u32, Vec<u8>), String>),
    PickerToggleView,
    DragEnter,
    DragLeave,
    /// A file drag moved over the editor (window coordinates).
    DragMotion(f32, f32),
    Dropped(Option<UriList>),
    ImageLoaded(String, Result<Processed, String>),
    ImageMenu(Option<usize>),
    SetFrame(usize, FrameStyle),
    SetAlign(usize, Align),
    SetWidth(usize, Option<u32>),
    SetCaption(usize, String),
    RemoveImage(usize),
    ResizeStart(usize),
    /// Left button went down on a picture: a click (menu) or the start of a drag.
    ImagePress(usize),
    /// The pointer entered (true) or left (false) a scroll pane.
    ScrollHover(ScrollArea, bool),
    MousePressed,
    /// Flush the note to disk and leave.
    Quit,
    /// Left button went down on a sidebar tag row or spacer: the start of a
    /// drag. The index is into the root entry list (`root_entries`).
    TagPress(usize),
    /// While dragging a sidebar entry, the pointer is over the slot before
    /// this root entry.
    TagDragOver(usize),
    /// Hovering the add-a-spacer strip at the bottom of the tags pane.
    SpaceHint(bool),
    /// Append a spacer line to the tag list.
    AddSpace,
    /// Remove the spacer at this root-entry index (right-click).
    RemoveSpace(usize),
    /// The pane the font picker acts on.
    SetFontTarget(Pane),
    /// Set the editor body weight (200 | 300 | 400 | 500).
    SetWeight(u16),
    /// Nudge the UI glide time by this many stepper clicks (`±1`).
    AnimStep(i16),
    /// Nudge the glide's landing softness by this many clicks (`±1`).
    AnimEaseStep(i16),
    /// A frame of the drop-indicator glide.
    AnimTick(Instant),
    /// Left button went down on a link card: the start of a drag (a
    /// double-click opens the target instead).
    LinkPress(usize),
    /// While dragging, the pointer is over the slot before this body line.
    DragOver(usize),
    MouseMoved(Point),
    MouseReleased,
    OpenImage(String),
    FontLoaded,
    /// The window exists (so the compositor can register fonts): load the bundled faces.
    LoadFonts,

    Editor(usize, text_editor::Action),
    Undo,
    Redo,
    ToggleShortcuts,
    /// The neon coffee sign.
    ToggleCoffee,
    CoffeeTick,
    ToggleSavedInfo,
    SavedInfoTick,
    AutosaveTick,
    SetView(View),
    Select(String),
    NewNote,
    TrashCurrent,
    RestoreCurrent,
    DeleteCurrentForever,
    EmptyTrash,
    TogglePin,
    Search(String),
    FocusSearch,
    Format(Format),
    /// A middle-button sweep over text: the highlighter pen.
    MarkSweep,
    ToggleDock,
    NewFolderName(String),
    CreateFolder,
    TakeScreenshot,
    ScriptTick,
    ScreenshotTaken(Arc<cosmic::iced::window::Screenshot>),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "io.github.reddemo0110.Attic";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let config_handler = cosmic_config::Config::new(Self::APP_ID, Config::VERSION).ok();
        let mut config = config_handler
            .as_ref()
            .map(|ctx| match Config::get_entry(ctx) {
                Ok(config) => config,
                Err((errors, config)) => {
                    for why in errors {
                        tracing::warn!(%why, "error loading app config");
                    }
                    config
                }
            })
            .unwrap_or_default();

        if config.device_id.is_empty() {
            let id = crate::note::new_id();
            if let Some(handler) = &config_handler
                && let Err(why) = config.set_device_id(handler, id.clone())
            {
                tracing::warn!(%why, "could not persist device id");
            }
            config.device_id = id;
        }

        let (store, store_error) = match Store::open(
            config.notes_dir(),
            &Config::index_path(Self::APP_ID),
            config.device_id.clone(),
        ) {
            Ok(store) => (Some(store), None),
            Err(err) => {
                tracing::error!(%err, "opening store");
                (None, Some(format!("{err:#}")))
            }
        };

        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        let theme = retro::Theme::from_key(&config.theme);
        let buffet_on = config.buffet_on;
        let buffet_light_mode = config.buffet_mode == "light";
        let buffet_highlight = buffet_highlight_from_config(&config.buffet_highlight);
        let buffet_dark = retro::Dark::from_key(&config.buffet_dark);
        let buffet_light = retro::Light::from_key(&config.buffet_light);
        let buffet_ink = retro::Ink::from_key(&config.buffet_ink);
        let show_markers = config.show_markers;
        let (show_nav, show_list) = (!config.hide_nav, !config.hide_list);
        let editor_font = retro::EditorFont::from_key(&config.editor_font);
        let editor_weight = match config.editor_weight {
            w @ (200 | 300 | 500) => w,
            _ => 400,
        };
        let ui_font = retro::EditorFont::from_key(&config.ui_font);
        let list_font = if config.list_font.is_empty() {
            ui_font
        } else {
            retro::EditorFont::from_key(&config.list_font)
        };
        let title_font = title_font_from_config(&config.title_font);
        let font_size = size_from_config(
            config.editor_font_size,
            retro::FONT_SIZE_DEFAULT,
            retro::FONT_SIZE_MIN,
            retro::FONT_SIZE_MAX,
        );
        let sidebar_size = pane_size_from_config(config.sidebar_font_size);
        let list_size = pane_size_from_config(config.list_font_size);
        let dock_size = retro::DockSize::from_key(&config.dock_size);
        let anim_ms = crate::anim::ms_from_key(&config.animation);
        let anim_ease10 = crate::anim::ease_from_key(&config.animation_ease);
        let task_marker = task_marker_from_config(&config.task_marker);
        let measure = retro::Measure::from_key(&config.text_width);
        let rule_size = retro::LineSize::from_key(&config.rule_size);
        let card_line = retro::LineSize::from_key(&config.card_line);
        let tag_line_size = retro::LineSize::from_key(&config.tag_line_size);
        let card_border = !config.card_border_off;
        let icon_theme =
            (!config.icon_theme.is_empty()).then(|| retro::Theme::from_key(&config.icon_theme));
        let coffee_unlocked = config.coffee_unlocked;
        let tag_icons = Arc::new(crate::glyph::parse_assignments(&config.tag_icons));
        let icon_set = crate::glyph::IconSet::from_key(&config.icon_set);
        let link_previews = !config.link_previews_off;
        let collapsed: HashSet<String> = config.collapsed_tags.iter().cloned().collect();
        let sync_ui = SyncUi {
            url: config.sync_url.clone(),
            email: config.sync_email.clone(),
            ..SyncUi::default()
        };
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            key_binds: key_binds(),
            config,
            config_handler,
            theme,
            buffet_on,
            buffet_light_mode,
            buffet_highlight,
            buffet_dark,
            buffet_light,
            buffet_ink,
            show_markers,
            show_nav,
            show_list,
            editor_font,
            editor_weight,
            ui_font,
            list_font,
            title_font,
            font_target: Pane::Editor,
            font_size,
            sidebar_size,
            list_size,
            dock_size,
            task_marker,
            measure,
            icon_theme,
            appearance_open: [false; 9],
            sync: sync_ui,
            image_cache: HashMap::new(),
            image_menu: None,
            task_space: None,
            resizing: None,
            table_edit: None,
            table_draft: String::new(),
            table_touched: false,
            table_input: widget::Id::unique(),
            table_resize: None,
            table_pick: None,
            table_fill: None,
            table_sel: None,
            rule_size,
            card_line,
            tag_line_size,
            card_border,
            win_size: None,
            dragging: None,
            scroll_hover: None,
            scroll_hover_release: false,
            mouse_down: false,
            tag_drag: None,
            space_hint: false,
            anim_ms,
            anim_ease10,
            tag_line: crate::anim::Glide::default(),
            tag_rows: std::cell::RefCell::new(Vec::new()),
            tag_area: std::rc::Rc::default(),
            note_line: crate::anim::Glide::default(),
            note_slots: std::cell::RefCell::new(std::collections::HashMap::new()),
            note_area: std::rc::Rc::default(),
            anim_last: None,
            show_shortcuts: false,
            coffee: None,
            coffee_seed: 7,
            coffee_unlocked,
            mouse_x: 0.0,
            mouse_y: 0.0,
            live_width: None,
            picker_dir: dirs::picture_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("/")),
            picker_entries: Vec::new(),
            thumbs: HashMap::new(),
            picker_grid: true,
            picker_files: false,
            previews: HashMap::new(),
            link_menu: None,
            link_previews,
            drop_hover: false,
            drop_target: None,
            store,
            store_error,
            view: View::All,
            view_counts: [0; 3],
            collapsed,
            tag_menu: None,
            tag_rename: None,
            tag_icon_pick: None,
            tag_icons,
            icon_set,
            rename_id: widget::Id::unique(),
            tags: Vec::new(),
            query: String::new(),
            search_id: widget::Id::unique(),
            notes_scroll_id: widget::Id::unique(),
            editor_scroll_id: widget::Id::unique(),
            dock_open: false,
            new_folder: String::new(),
            folder_id: widget::Id::unique(),
            notes: Vec::new(),
            current: None,
            blocks: Blocks::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            save_error: None,
            saved_info_until: None,
            last_undo_kind: EditKind::Other,
            last_undo_at: Instant::now(),
            dirty: false,
            last_edit: Instant::now(),
            backlinks: Vec::new(),
            screenshot_pending: std::env::var_os("JJB_SCREENSHOT").is_some(),
            script: debug_script::Runner::from_env(),
        };

        app.refresh_tags();
        app.refresh_list();
        // Open the most recent note so the window isn't empty on launch.
        if let Some(first) = app.notes.first().map(|n| n.id.clone()) {
            app.open_note(&first);
        }

        // The Appearance drawer sits beside the note, not over it.
        app.core.window.context_is_overlay = false;
        app.install_icon();
        let title = app.update_title();
        // Fonts are (re)loaded on `window::Event::Opened` too: a LoadFont
        // action issued before the compositor exists is silently dropped.
        // A remembered account: fetch the token and pick up where we left off.
        let restore = if !app.sync.url.trim().is_empty() && !app.sync.email.trim().is_empty() {
            app.sync.restoring = true;
            Task::perform(
                async {
                    match crate::secrets::get(sync::TOKEN_KEY).await {
                        Ok(Some(bytes)) => String::from_utf8(bytes).ok(),
                        Ok(None) => None,
                        Err(err) => {
                            tracing::warn!(%err, "reading the sync token from the keyring");
                            None
                        }
                    }
                },
                |token| cosmic::Action::App(Message::SyncTokenLoaded(token)),
            )
        } else {
            Task::none()
        };
        // Whatever the command line asked for, now that the store is up.
        let asked = match flags.message() {
            Some(msg) => app.update(msg),
            None => Task::none(),
        };
        (app, Task::batch([load_fonts(), title, restore, asked]))
    }

    /// A second launch, or the GNOME Shell search provider, reached the
    /// running instance over D-Bus.
    fn dbus_activation(
        &mut self,
        msg: cosmic::dbus_activation::Message,
    ) -> Task<cosmic::Action<Self::Message>> {
        use cosmic::dbus_activation::Details;
        tracing::info!(details = ?msg.msg, token = msg.activation_token.is_some(), "activated over D-Bus");
        // libcosmic raises the window itself when the caller brought an
        // activation token; without one (a plain second launch, the
        // search provider) we ask the compositor ourselves.
        let raise = if msg.activation_token.is_none() {
            self.update(Message::Raise)
        } else {
            Task::none()
        };
        let action = match msg.msg {
            Details::Activate => Task::none(),
            Details::Open { url } => self.update(Message::OpenPaths(
                url.into_iter().map(|u| u.to_string()).collect(),
            )),
            Details::ActivateAction { action, args } => match action.as_str() {
                "open" => self.update(Message::OpenPaths(args)),
                "search" => self.update(Message::SearchFromOutside(args.join(" "))),
                "new-note" => self.update(Message::NewNote),
                other => {
                    tracing::warn!(action = other, "unknown activation action");
                    Task::none()
                }
            },
        };
        Task::batch([raise, action])
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        let mut style = retro::app_style(&self.palette());
        // With rounded corners (a floating window off COSMIC: Mutter and
        // KWin have no corner-clipping protocol) libcosmic draws the window
        // as a rounded container and leaves the surface behind it clear,
        // the way its own default style does. A palette colour there fills
        // the corner outside the curve with a square of near-black.
        if !self.core.window.sharp_corners && !self.core.window.is_maximized {
            style.background_color = cosmic::iced::Color::TRANSPARENT;
        }
        Some(style)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        // On GNOME the fonts/buttons come from the portal, and libcosmic
        // replaces its toolkit config underneath us when its own config
        // subscription reports; putting ours back here is cheap.
        crate::desktop::reassert();
        let view_items = vec![
            menu::Item::CheckBox(fl!("show-nav"), None, self.show_nav, MenuAction::ToggleNav),
            menu::Item::CheckBox(
                fl!("show-list"),
                None,
                self.show_list,
                MenuAction::ToggleList,
            ),
            menu::Item::Button(fl!("editor-only"), None, MenuAction::Solo),
            menu::Item::Divider,
            menu::Item::Button(fl!("zoom-in"), None, MenuAction::ZoomIn),
            menu::Item::Button(fl!("zoom-out"), None, MenuAction::ZoomOut),
            menu::Item::Button(fl!("zoom-reset"), None, MenuAction::ZoomReset),
            menu::Item::Divider,
            menu::Item::Button(fl!("theme-colours"), None, MenuAction::Themes),
            menu::Item::CheckBox(
                fl!("show-markers"),
                None,
                self.show_markers,
                MenuAction::ToggleMarkers,
            ),
            menu::Item::Divider,
            menu::Item::Button(fl!("shortcuts"), None, MenuAction::Shortcuts),
            menu::Item::Button(fl!("about"), None, MenuAction::About),
        ];

        let menu_bar = menu::bar(vec![
            menu::Tree::with_children(
                menu::root(fl!("file")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::Button(fl!("new-note"), None, MenuAction::NewNote),
                        menu::Item::Button(fl!("new-folder"), None, MenuAction::NewFolder),
                        menu::Item::Button(fl!("dock-image"), None, MenuAction::AddImage),
                        menu::Item::Button(fl!("dock-attach"), None, MenuAction::AttachFile),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("search-notes"), None, MenuAction::FocusSearch),
                        menu::Item::Button(fl!("pin-note"), None, MenuAction::Pin),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("trash-note"), None, MenuAction::TrashNote),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("quit"), None, MenuAction::Quit),
                    ],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("edit")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::Button(fl!("undo"), None, MenuAction::Undo),
                        menu::Item::Button(fl!("redo"), None, MenuAction::Redo),
                    ],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("format")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    Format::ALL
                        .into_iter()
                        .map(|f| menu::Item::Button(f.label(), None, MenuAction::Format(f)))
                        .collect(),
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("view")).apply(Element::from),
                menu::items(&self.key_binds, view_items),
            ),
        ]);

        let toggle = |icon_name: &'static str, label: String, on: bool, msg: Message| {
            widget::tooltip(
                widget::button::icon(icon::from_name(icon_name))
                    .selected(on)
                    .on_press(msg),
                widget::text::body(label),
                widget::tooltip::Position::Bottom,
            )
            .into()
        };
        vec![
            menu_bar.into(),
            toggle(
                "sidebar-places-symbolic",
                self.with_shortcut(fl!("show-nav"), MenuAction::ToggleNav),
                self.show_nav,
                Message::ToggleNav,
            ),
            toggle(
                "view-list-symbolic",
                self.with_shortcut(fl!("show-list"), MenuAction::ToggleList),
                self.show_list,
                Message::ToggleList,
            ),
        ]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        let in_trash = matches!(self.view, View::Trash);
        let sc = |label: String, action: MenuAction| self.with_shortcut(label, action);

        if let Some(note) = &self.current {
            if in_trash {
                items.push(header_button(
                    "edit-undo-symbolic",
                    fl!("restore-note"),
                    Message::RestoreCurrent,
                ));
                items.push(header_button(
                    "edit-delete-symbolic",
                    fl!("delete-forever"),
                    Message::DeleteCurrentForever,
                ));
            } else {
                let pin_label = if note.pinned {
                    fl!("unpin-note")
                } else {
                    fl!("pin-note")
                };
                items.push(header_button(
                    "pin-symbolic",
                    sc(pin_label, MenuAction::Pin),
                    Message::TogglePin,
                ));
                items.push(header_button(
                    "user-trash-symbolic",
                    sc(fl!("trash-note"), MenuAction::TrashNote),
                    Message::TrashCurrent,
                ));
            }
        }
        if in_trash && !self.notes.is_empty() {
            items.push(header_button(
                "edit-clear-all-symbolic",
                fl!("empty-trash"),
                Message::EmptyTrash,
            ));
        }
        if !in_trash {
            items.push(header_button(
                "list-add-symbolic",
                sc(fl!("new-note"), MenuAction::NewNote),
                Message::NewNote,
            ));
        }
        items
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        None
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
            ContextPage::Themes => context_drawer::context_drawer(
                self.theme_picker(),
                Message::ToggleContextPage(ContextPage::Themes),
            )
            .title(fl!("appearance")),
            ContextPage::Picker => context_drawer::context_drawer(
                self.file_picker(),
                Message::ToggleContextPage(ContextPage::Picker),
            )
            .title(fl!("pick-image")),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let p = self.palette();
        if let Some(err) = &self.store_error {
            return widget::container(retro::text(&p, fl!("store-error", error = err.as_str())))
                .padding(24)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into();
        }

        // Flat layout: panes butt against each other with hairlines between.
        let nav = widget::column::with_capacity(3)
            .push(
                widget::container(self.views_frame(&p))
                    // Three rows plus the header; grows with the sidebar text.
                    .height(Length::Fixed(
                        (f32::from(self.sidebar_size) * 1.3 + 8.0) * 3.0 + 66.0,
                    ))
                    .width(Length::Fill),
            )
            .push(retro::hrule(&p))
            .push(self.tags_frame(&p))
            .width(Length::Fixed(NAV_WIDTH))
            .height(Length::Fill);

        let list = widget::container(self.notes_frame(&p))
            .width(Length::Fixed(NOTE_LIST_WIDTH))
            .height(Length::Fill);

        let mut editor_col = widget::column::with_capacity(3)
            .push(self.editor_frame(&p))
            .width(Length::Fill)
            .height(Length::Fill);
        if !self.backlinks.is_empty() {
            editor_col = editor_col.push(retro::hrule(&p)).push(
                // Pane padding + title bar + one row of chips + scrollbar.
                widget::container(self.backlinks_frame(&p))
                    .height(Length::Fixed(88.0))
                    .width(Length::Fill),
            );
        }

        let mut panes = widget::row::with_capacity(5)
            .width(Length::Fill)
            .height(Length::Fill);
        if self.show_nav {
            panes = panes.push(nav).push(retro::vrule(&p));
        }
        if self.show_list {
            panes = panes.push(list).push(retro::vrule(&p));
        }
        panes = panes.push(editor_col);

        let content = widget::container(panes)
            .width(Length::Fill)
            .height(Length::Fill);
        let mut layers: Vec<Element<'_, Message>> = vec![content.into()];
        if self.show_shortcuts {
            layers.push(self.shortcuts_overlay(&p));
        }
        if self.coffee.is_some() {
            layers.push(self.coffee_overlay(&p));
        }
        if layers.len() == 1 {
            layers.pop().expect("content")
        } else {
            cosmic::iced::widget::stack(layers)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            event::listen_with(|event, status, _window| match event {
                // Arrows nobody consumed (the editor is not focused) walk the list.
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::ArrowDown),
                    modifiers,
                    ..
                }) if status == event::Status::Ignored && modifiers.is_empty() => {
                    Some(Message::NavigateNotes(1))
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::ArrowUp),
                    modifiers,
                    ..
                }) if status == event::Status::Ignored && modifiers.is_empty() => {
                    Some(Message::NavigateNotes(-1))
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    physical_key,
                    ..
                }) => Some(Message::Key(modifiers, key, physical_key)),
                Event::Window(window::Event::FileDropped(paths)) => {
                    Some(Message::ImagesDropped(paths))
                }
                Event::Window(window::Event::Opened { .. }) => Some(Message::LoadFonts),
                Event::Window(window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size.width, size.height))
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::MouseMoved(position))
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    Some(Message::MousePressed)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::MouseReleased)
                }
                _ => None,
            }),
        ];

        if self.tag_line.live || self.note_line.live {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(15)).map(Message::AnimTick),
            );
        }
        if self.dirty {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(200))
                    .map(|_| Message::AutosaveTick),
            );
        }
        // Ticks only while a resize is unsaved; one write and it is gone.
        if self
            .win_size
            .is_some_and(|(w, h)| (w, h) != (self.config.window_width, self.config.window_height))
        {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(1000))
                    .map(|_| Message::WindowSizeTick),
            );
        }
        if self.sync.session.is_some() {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(1000)).map(|_| Message::SyncTick),
            );
        }
        if self.coffee.is_some() {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(170)).map(|_| Message::CoffeeTick),
            );
        }
        if self.saved_info_until.is_some() {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(500))
                    .map(|_| Message::SavedInfoTick),
            );
        }
        if self.screenshot_pending {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(2500))
                    .map(|_| Message::TakeScreenshot),
            );
        }
        if self
            .script
            .as_ref()
            .is_some_and(debug_script::Runner::is_active)
        {
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_millis(100)).map(|_| Message::ScriptTick),
            );
        }

        subscriptions.push(crate::desktop::watch().map(Message::DesktopSettings));

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        let task = self.update_inner(message);
        let mut tasks = vec![task];
        tasks.extend(self.image_loads());
        // Off COSMIC, libcosmic installs a fresh system theme at start-up
        // and on every dark/light or accent change, outside any message of
        // ours; the redraw event does not reach subscriptions, so the next
        // message (start-up has plenty, and every pointer move is one)
        // puts the desktop's window outline back.
        if crate::desktop::outline_stale() {
            tasks.push(self.update_inner(Message::SoftenOutline));
        }
        if tasks.len() == 1 {
            tasks.pop().unwrap_or_else(Task::none)
        } else {
            Task::batch(tasks)
        }
    }

    fn on_escape(&mut self) -> Task<cosmic::Action<Self::Message>> {
        if self.show_shortcuts {
            self.show_shortcuts = false;
        } else if self.dock_open {
            self.dock_open = false;
        } else if self.core.window.show_context {
            self.core.window.show_context = false;
        } else if !self.query.is_empty() {
            self.query.clear();
            self.refresh_list();
        }
        Task::none()
    }

    fn on_app_exit(&mut self) -> Option<Self::Message> {
        self.close_current();
        None
    }
}

impl AppModel {
    fn update_inner(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::ImagesDropped(paths) => {
                let mut task = Task::none();
                for path in paths {
                    task = if images::is_image_file(&path) {
                        self.import_image(&path)
                    } else if path.is_file() {
                        self.import_file_at(&path, None)
                    } else {
                        task
                    };
                }
                return task;
            }

            Message::PickImage | Message::PickFile => {
                if self.current.as_ref().is_none_or(|n| n.trashed) {
                    return Task::none();
                }
                self.picker_files = matches!(message, Message::PickFile);
                self.picker_entries =
                    images::list_dir_filtered(&self.picker_dir, self.picker_files);
                self.context_page = ContextPage::Picker;
                self.core.window.show_context = true;
                return self.load_thumbs();
            }

            Message::PickerNavigate(dir) => {
                self.picker_dir = dir;
                self.picker_entries =
                    images::list_dir_filtered(&self.picker_dir, self.picker_files);
                return self.load_thumbs();
            }

            Message::ThumbLoaded(path, result) => {
                let state = match result {
                    Ok((w, h, rgba)) => {
                        ImageState::Ready(widget::image::Handle::from_rgba(w, h, rgba), w, h)
                    }
                    Err(err) => ImageState::Failed(err),
                };
                self.thumbs.insert(path, state);
            }

            Message::PickerToggleView => self.picker_grid = !self.picker_grid,

            Message::PickerChoose(path) => {
                self.core.window.show_context = false;
                return if images::is_image_file(&path) {
                    self.import_image(&path)
                } else {
                    self.import_file_at(&path, None)
                };
            }

            Message::PreviewLoaded(url, result) => {
                let state = match result {
                    Ok(p) => {
                        let handle = p.image.as_ref().map(widget::image::Handle::from_path);
                        // A bare address learns its title, so the note itself
                        // carries it (and the cache is only a speed-up).
                        if !p.title.is_empty() {
                            let mut changed = false;
                            for (_, l) in self.blocks.items.iter_mut().enumerate().filter_map(
                                |(i, b)| match b {
                                    Block::Link(l) if l.target == url && l.text.is_empty() => {
                                        Some((i, l))
                                    }
                                    _ => None,
                                },
                            ) {
                                l.text = p.title.replace(['[', ']'], "");
                                changed = true;
                            }
                            if changed {
                                self.dirty = true;
                                self.last_edit = Instant::now();
                            }
                        }
                        PreviewState::Ready(p, handle)
                    }
                    Err(err) => PreviewState::Failed(err),
                };
                self.previews.insert(url, state);
            }

            Message::LinkMenu(block) => {
                self.link_menu = block;
                self.image_menu = None;
            }

            Message::RemoveLink(block) => {
                self.link_menu = None;
                self.record(EditKind::Other);
                self.blocks.remove_block(block);
                self.dirty = true;
                self.last_edit = Instant::now();
                return self.focus_editor();
            }

            Message::CopyLink(url) => {
                self.link_menu = None;
                return cosmic::iced::clipboard::write(url);
            }

            Message::RefreshPreview(url) => {
                self.link_menu = None;
                if let Some(store) = &self.store {
                    crate::links::forget(store.notes_dir(), &url);
                }
                self.previews.remove(&url);
                return self.queue_previews();
            }

            Message::OpenFile(rel) => {
                self.link_menu = None;
                if let Some(store) = &self.store {
                    let path = images::resolve(store.notes_dir(), &rel);
                    if let Err(err) = open::that_detached(&path) {
                        tracing::warn!(%err, "opening attached file");
                    }
                }
            }

            Message::SyncUrl(v) => self.sync.url = v,
            Message::SyncEmail(v) => self.sync.email = v,
            Message::SyncPassword(v) => self.sync.password = v,
            Message::SyncShowPassword => self.sync.show_password = !self.sync.show_password,

            Message::SyncSignIn | Message::SyncSignUp => {
                if self.sync.busy {
                    return Task::none();
                }
                let signup = matches!(message, Message::SyncSignUp);
                let (url, email, password) = (
                    self.sync.url.clone(),
                    self.sync.email.clone(),
                    self.sync.password.clone(),
                );
                self.sync.busy = true;
                self.sync.error = None;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            if signup {
                                sync::sign_up(&url, &email, &password)
                            } else {
                                sync::sign_in(&url, &email, &password)
                            }
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| format!("{e:#}")))
                    },
                    |result| cosmic::Action::App(Message::SyncAuth(result)),
                );
            }

            Message::SyncTokenLoaded(token) => {
                self.sync.restoring = false;
                let Some(token) = token else {
                    self.sync.error = Some(fl!("sync-signed-out"));
                    return Task::none();
                };
                let session = Session {
                    url: self.sync.url.trim().trim_end_matches('/').to_owned(),
                    token,
                    user_id: String::new(),
                    email: self.sync.email.clone(),
                };
                self.sync.busy = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || sync::refresh(&session))
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r.map_err(|e| format!("{e:#}")))
                            .and_then(|s| s.ok_or_else(|| fl!("sync-signed-out")))
                    },
                    |result| cosmic::Action::App(Message::SyncAuth(result)),
                );
            }

            Message::SyncAuth(result) => {
                self.sync.busy = false;
                match result {
                    Ok(session) => {
                        self.sync.password.clear();
                        self.sync.error = None;
                        self.sync.url.clone_from(&session.url);
                        self.sync.email.clone_from(&session.email);
                        if let Some(handler) = &self.config_handler {
                            if let Err(why) = self.config.set_sync_url(handler, session.url.clone()) {
                                tracing::warn!(%why, "saving sync server");
                            }
                            if let Err(why) = self.config.set_sync_email(handler, session.email.clone()) {
                                tracing::warn!(%why, "saving sync email");
                            }
                        }
                        if let Some(store) = self.store.as_mut()
                            && let Err(err) = store.set_sync_account(&session.account())
                        {
                            tracing::error!(%err, "binding sync state to the account");
                        }
                        let token = session.token.clone();
                        self.sync.session = Some(session);
                        self.sync.due = Some(Instant::now());
                        return Task::perform(
                            async move {
                                if let Err(err) =
                                    crate::secrets::store(sync::TOKEN_KEY, "Attic sync", token.as_bytes())
                                        .await
                                {
                                    tracing::warn!(%err, "keeping the sync token in the keyring");
                                }
                            },
                            |()| cosmic::Action::None,
                        );
                    }
                    Err(err) => self.sync.error = Some(err),
                }
            }

            Message::SyncSignOut => {
                self.sync.session = None;
                self.sync.error = None;
                self.sync.last_ok = None;
                self.sync.summary = None;
                return Task::perform(
                    async {
                        if let Err(err) = crate::secrets::delete(sync::TOKEN_KEY).await {
                            tracing::warn!(%err, "removing the sync token from the keyring");
                        }
                    },
                    |()| cosmic::Action::None,
                );
            }

            Message::SyncNow => {
                self.sync.due = Some(Instant::now());
                return self.start_sync();
            }

            Message::SyncTick => {
                let due = self.sync.due.is_some_and(|t| t <= Instant::now());
                let stale = self
                    .sync
                    .last_run
                    .is_none_or(|t| t.elapsed() >= SYNC_INTERVAL);
                if due || stale {
                    return self.start_sync();
                }
            }

            Message::SyncDone(out) => {
                self.sync.busy = false;
                if out.unauthorized {
                    let task = self.update(Message::SyncSignOut);
                    self.sync.error = Some(fl!("sync-signed-out"));
                    return task;
                }
                let mut token_task = Task::none();
                if let (Some(fresh), Some(session)) = (&out.session, self.sync.session.as_mut())
                    && fresh.token != session.token
                {
                    session.token.clone_from(&fresh.token);
                    session.user_id = fresh.user_id.clone();
                    let token = fresh.token.clone();
                    token_task = Task::perform(
                        async move {
                            if let Err(err) =
                                crate::secrets::store(sync::TOKEN_KEY, "Attic sync", token.as_bytes())
                                    .await
                            {
                                tracing::warn!(%err, "keeping the sync token in the keyring");
                            }
                        },
                        |()| cosmic::Action::None,
                    );
                }
                let current_id = self.current.as_ref().map(|n| n.id.clone());
                // A renamed file rewrites the notes that show it — maybe
                // this one.
                let touches_open = !out.files.renamed.is_empty()
                    || current_id
                        .as_ref()
                        .is_some_and(|id| out.incoming.iter().any(|r| &r.note_id == id));
                if touches_open {
                    // Unsaved typing becomes a local change first, so it is
                    // kept (as a conflict copy if need be) rather than lost.
                    self.commit_table_edit();
                    self.flush();
                }
                let device = sync::device_name();
                let applied = match self.store.as_mut() {
                    Some(store) => store.apply_outcome(&out, &device),
                    None => Vec::new(),
                };
                let conflicts = applied
                    .iter()
                    .filter(|(_, a)| matches!(a, Applied::Conflict { .. }))
                    .count();
                let changed = applied
                    .iter()
                    .filter(|(_, a)| !matches!(a, Applied::Unchanged | Applied::Repointed))
                    .count();
                let repointed = applied.iter().any(|(_, a)| matches!(a, Applied::Repointed));
                if let Some(id) = current_id {
                    for (_, what) in applied.iter().filter(|(nid, _)| *nid == id) {
                        match what {
                            Applied::Deleted => {
                                self.current = None;
                                self.blocks = Blocks::default();
                            }
                            // The open note follows the server (or its
                            // renamed picture): reload it in place.
                            Applied::Adopted | Applied::Conflict { .. } | Applied::Repointed => {
                                self.open_note(&id);
                            }
                            Applied::Unchanged => {}
                        }
                    }
                }
                if changed > 0 || repointed || !out.pushed.is_empty() || out.files.folders_remote.is_some() {
                    self.after_store_change();
                }
                // A picture that was missing when its note opened may be
                // here now: give the failures another go.
                let mut image_tasks = Vec::new();
                if out.files.down > 0 {
                    self.image_cache.retain(|_, state| !matches!(state, ImageState::Failed(_)));
                    image_tasks = self.image_loads();
                }
                self.sync.last_run = Some(Instant::now());
                self.sync.due = None;
                let first_error = out.errors.first().or_else(|| out.files.errors.first());
                if let Some(err) = first_error {
                    tracing::warn!(errors = ?out.errors, files = ?out.files.errors, "sync cycle had errors");
                    self.sync.error = Some(err.clone());
                } else {
                    self.sync.error = None;
                    self.sync.last_ok = Some(Instant::now());
                }
                self.sync.summary = if conflicts > 0 {
                    Some(fl!("sync-conflicts", n = conflicts, device = device))
                } else if changed > 0 || !out.pushed.is_empty() {
                    Some(fl!("sync-moved", down = changed, up = out.pushed.len()))
                } else {
                    None
                };
                self.sync.files_summary = (out.files.down > 0 || out.files.up > 0)
                    .then(|| fl!("sync-files-moved", down = out.files.down, up = out.files.up));
                // Conflict copies and repointed notes are local changes, and
                // a big assets folder goes up a slice at a time: go again.
                if conflicts > 0 || repointed || out.files.more {
                    self.sync.due = Some(Instant::now() + Duration::from_secs(1));
                }
                image_tasks.push(token_task);
                image_tasks.push(self.update_title());
                return Task::batch(image_tasks);
            }

            Message::SetLinkPreviews(on) => {
                self.link_previews = on;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_link_previews_off(handler, !on)
                {
                    tracing::warn!(%why, "saving link preview setting");
                }
                if on {
                    return self.queue_previews();
                }
            }

            Message::DragEnter => self.drop_hover = true,
            Message::DragLeave => {
                self.drop_hover = false;
                self.drop_target = None;
            }
            Message::DragMotion(x, y) => {
                self.drop_hover = true;
                let target = self.drop_line_at(x, y);
                tracing::debug!(
                    offer = ?(x, y),
                    pointer = ?self.blocks.items.iter().find_map(|b| match b {
                        Block::Text { content, .. } => content.pointer_y(),
                        _ => None,
                    }),
                    target,
                    "drag over note"
                );
                self.drop_target = Some(target);
            }

            Message::Dropped(list) => {
                self.drop_hover = false;
                let target = self.drop_target.take();
                let mut task = Task::none();
                if let Some(UriList(paths)) = list {
                    for path in paths {
                        task = if images::is_image_file(&path) {
                            self.import_image_at(&path, target)
                        } else if path.is_file() {
                            self.import_file_at(&path, target)
                        } else {
                            task
                        };
                    }
                }
                return task;
            }

            Message::ImageLoaded(key, result) => {
                let state = match result {
                    Ok(Processed::Pixels {
                        width,
                        height,
                        rgba,
                    }) => ImageState::Ready(
                        widget::image::Handle::from_rgba(width, height, rgba),
                        width,
                        height,
                    ),
                    Ok(Processed::Ascii(text)) => ImageState::Ascii(text),
                    Err(err) => ImageState::Failed(err),
                };
                self.image_cache.insert(key, state);
            }

            Message::ImageMenu(block) => {
                self.image_menu = if self.image_menu == block {
                    None
                } else {
                    block
                };
            }

            Message::SetFrame(block, frame) => {
                self.image_menu = None;
                self.edit_ref(block, |r| {
                    r.frame = frame;
                    // Plain means the picture as it is, across the text
                    // column: it drops any width or float it had.
                    if frame == FrameStyle::Plain {
                        r.width = None;
                        r.align = Align::Center;
                    }
                });
            }

            Message::SetAlign(block, align) => {
                self.image_menu = None;
                self.edit_ref(block, |r| r.align = align);
            }

            Message::SetWidth(block, width) => {
                self.image_menu = None;
                self.edit_ref(block, |r| r.width = width);
            }

            Message::SetCaption(block, caption) => {
                let caption = caption.replace(['[', ']', '\n'], "");
                self.edit_ref_kind(block, EditKind::Typing, |r| r.alt = caption);
            }

            Message::SetFontTarget(pane) => self.font_target = pane,

            Message::SetFont(font) => {
                let saved = match self.font_target {
                    Pane::Editor => {
                        self.editor_font = font;
                        self.config_handler.as_ref().map(|h| {
                            self.config.set_editor_font(h, font.key().to_owned())
                        })
                    }
                    Pane::Sidebar => {
                        self.ui_font = font;
                        self.config_handler.as_ref().map(|h| {
                            self.config.set_ui_font(h, font.key().to_owned())
                        })
                    }
                    Pane::List => {
                        self.list_font = font;
                        self.config_handler.as_ref().map(|h| {
                            self.config.set_list_font(h, font.key().to_owned())
                        })
                    }
                };
                if let Some(Err(why)) = saved {
                    tracing::warn!(%why, "saving font choice");
                }
            }

            Message::RevealCaret(rect) => {
                let margin = f32::from(self.font_size) * 2.0;
                return cosmic::iced::runtime::task::effect(cosmic::iced::runtime::Action::widget(
                    reveal_in_scrollable(self.editor_scroll_id.clone(), rect, margin),
                ));
            }

            Message::FollowLink(link) => match link {
                crate::editor::widget::Link::Note(title) => {
                    let found = self
                        .store
                        .as_ref()
                        .and_then(|s| s.find_by_title(&title).ok().flatten());
                    if let Some(id) = found {
                        return self.update(Message::Select(id));
                    }
                    // A link to a note that does not exist yet creates it,
                    // the way every wiki does; the link then resolves.
                    let title = title.trim();
                    if title.is_empty() || self.current.as_ref().is_some_and(|n| n.trashed) {
                        return Task::none();
                    }
                    tracing::info!(title, "no note with that title; creating it");
                    return self.create_note(format!("# {title}"), true);
                }
                crate::editor::widget::Link::Tag(tag) => {
                    if let Some(tag) = note::normalize_tag(tag.trim_start_matches('#')) {
                        return self.update(Message::SetView(View::Tag(tag)));
                    }
                }
            },

            Message::SetPairing(key) => {
                if let Some(pair) = retro::Pairing::from_key(&key) {
                    self.apply_pairing(pair);
                }
            }

            Message::RestoreFonts => {
                self.apply_pairing(retro::Pairing::default_pairing());
            }

            Message::SizeStep(pane, step) => {
                let (current, min, max) = match pane {
                    Pane::Editor => (self.font_size, retro::FONT_SIZE_MIN, retro::FONT_SIZE_MAX),
                    Pane::Sidebar => (
                        self.sidebar_size,
                        retro::PANE_SIZE_MIN,
                        retro::PANE_SIZE_MAX,
                    ),
                    Pane::List => (self.list_size, retro::PANE_SIZE_MIN, retro::PANE_SIZE_MAX),
                };
                let next = (i32::from(current) + i32::from(step))
                    .clamp(i32::from(min), i32::from(max)) as u16;
                self.set_pane_size(pane, next);
            }

            Message::SizeReset(pane) => {
                let default = match pane {
                    Pane::Editor => retro::FONT_SIZE_DEFAULT,
                    Pane::Sidebar | Pane::List => retro::PANE_SIZE_DEFAULT,
                };
                self.set_pane_size(pane, default);
            }

            Message::SetIcon(choice) => {
                self.icon_theme = choice;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_icon_theme(
                        handler,
                        choice.map_or(String::new(), |t| t.key().to_owned()),
                    )
                {
                    tracing::warn!(%why, "saving icon choice");
                }
                self.install_icon();
            }

            Message::SetMeasure(measure) => {
                self.measure = measure;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self
                        .config
                        .set_text_width(handler, measure.key().to_owned())
                {
                    tracing::warn!(%why, "saving text width");
                }
            }

            Message::SetTaskMarker(marker) => {
                self.task_marker = marker.clone();
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_task_marker(handler, marker)
                {
                    tracing::warn!(%why, "saving task marker");
                }
            }

            Message::ToggleSection(section) => {
                let i = section as usize;
                self.appearance_open[i] = !self.appearance_open[i];
            }

            Message::SetDockSize(size) => {
                self.dock_size = size;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_dock_size(handler, size.key().to_owned())
                {
                    tracing::warn!(%why, "saving dock size");
                }
            }

            Message::TagMenu(tag) => {
                self.tag_menu = tag;
                self.tag_rename = None;
                self.tag_icon_pick = None;
            }

            Message::SetIconSet(set) => {
                self.icon_set = set;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_icon_set(handler, set.key().to_owned())
                {
                    tracing::warn!(%why, "saving icon set");
                }
            }

            Message::TagIconPick(tag) => {
                self.tag_menu = None;
                self.tag_rename = None;
                self.tag_icon_pick = Some(tag);
            }

            Message::SetTagIcon(tag, icon) => {
                self.tag_icon_pick = None;
                let mut map = (*self.tag_icons).clone();
                match icon {
                    Some(i) => {
                        map.insert(tag, i);
                    }
                    None => {
                        map.remove(&tag);
                    }
                }
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self
                        .config
                        .set_tag_icons(handler, crate::glyph::serialise_assignments(&map))
                {
                    tracing::warn!(%why, "saving folder icons");
                }
                self.tag_icons = Arc::new(map);
            }

            Message::TagRenameStart(tag) => {
                self.tag_menu = None;
                self.tag_rename = Some((tag.clone(), tag));
                return widget::text_input::focus(self.rename_id.clone());
            }

            Message::TagRenameInput(draft) => {
                if let Some(r) = &mut self.tag_rename {
                    r.1 = draft;
                }
            }

            Message::TagRenameCancel => {
                self.tag_rename = None;
                self.tag_menu = None;
                self.tag_icon_pick = None;
            }

            Message::TagRenameCommit => {
                let Some((old, draft)) = self.tag_rename.take() else {
                    return Task::none();
                };
                let Some(new) = note::normalize_tag(&draft) else {
                    return Task::none();
                };
                if new == old {
                    return Task::none();
                }
                // Files are rewritten on disk: the open note must be saved first.
                self.flush();
                let under =
                    |t: &str| t == old || (t.starts_with(&old) && t[old.len()..].starts_with('/'));
                let remap = |t: &str| format!("{new}{}", &t[old.len()..]);
                let current_affected = self
                    .current
                    .as_ref()
                    .is_some_and(|n| note::extract_tags(&n.body).iter().any(|t| under(t)));
                let Some(store) = self.store.as_mut() else {
                    return Task::none();
                };
                match store.rename_tag(&old, &new) {
                    Ok(n) => tracing::info!(old, new, notes = n, "renamed tag"),
                    Err(err) => {
                        tracing::error!(%err, old, new, "renaming tag");
                        return Task::none();
                    }
                }
                if let View::Tag(t) = &self.view
                    && under(t)
                {
                    self.view = View::Tag(remap(t));
                }
                self.collapsed = self
                    .collapsed
                    .iter()
                    .map(|t| if under(t) { remap(t) } else { t.clone() })
                    .collect();
                if current_affected && let Some(id) = self.current.as_ref().map(|n| n.id.clone()) {
                    self.open_note(&id);
                }
                self.refresh_tags();
                self.refresh_list();
            }

            Message::ToggleTagFold(tag) => {
                if !self.collapsed.remove(&tag) {
                    self.collapsed.insert(tag);
                }
                if let Some(handler) = &self.config_handler {
                    let mut list: Vec<String> = self.collapsed.iter().cloned().collect();
                    list.sort();
                    if let Err(why) = self.config.set_collapsed_tags(handler, list) {
                        tracing::warn!(%why, "saving folded tags");
                    }
                }
            }

            Message::ToggleCoffee => {
                self.coffee = if self.coffee.is_some() {
                    None
                } else {
                    Some((0, 1.0))
                };
            }

            Message::CoffeeTick => {
                if let Some((frame, _)) = self.coffee {
                    // A cheap LCG: every so often a tube dims, like a real sign.
                    self.coffee_seed = self
                        .coffee_seed
                        .wrapping_mul(1_103_515_245)
                        .wrapping_add(12_345);
                    let roll = (self.coffee_seed >> 16) % 13;
                    let glow = match roll {
                        0 => 0.35,
                        1 => 0.7,
                        _ => 1.0,
                    };
                    self.coffee = Some(((frame + 1) % crate::coffee::FRAMES.len(), glow));
                }
            }

            Message::ToggleShortcuts => {
                self.show_shortcuts = !self.show_shortcuts;
            }

            Message::ToggleSavedInfo => {
                self.saved_info_until = match self.saved_info_until {
                    Some(_) => None,
                    None => Some(Instant::now() + Duration::from_secs(4)),
                };
            }

            Message::SavedInfoTick => {
                if self.saved_info_until.is_some_and(|t| Instant::now() >= t) {
                    self.saved_info_until = None;
                }
            }

            Message::Undo => {
                if let Some(snap) = self.undo.pop() {
                    let now = self.snapshot();
                    self.redo.push(now);
                    self.restore(snap);
                    return self.focus_editor();
                }
            }

            Message::Redo => {
                if let Some(snap) = self.redo.pop() {
                    let now = self.snapshot();
                    self.undo.push(now);
                    self.restore(snap);
                    return self.focus_editor();
                }
            }

            Message::RemoveImage(block) => {
                self.image_menu = None;
                self.record(EditKind::Other);
                self.blocks.remove_image(block);
                self.dirty = true;
                self.last_edit = Instant::now();
                return self.focus_editor();
            }

            Message::ResizeStart(block) => {
                self.image_menu = None;
                let start_w = self
                    .blocks
                    .images()
                    .iter()
                    .find(|(b, _)| *b == block)
                    .and_then(|(_, r)| r.width)
                    .unwrap_or(420);
                self.resizing = Some(Resize {
                    block,
                    start_x: self.mouse_x,
                    start_w,
                });
            }

            Message::ImagePress(block) => {
                let editable = self.current.as_ref().is_some_and(|n| !n.trashed);
                if self.image_menu.is_some() || !editable {
                    return self.update(Message::ImageMenu(Some(block)));
                }
                self.dragging = Some(ImageDrag {
                    block,
                    start: Point::new(self.mouse_x, self.mouse_y),
                    active: false,
                    target: None,
                });
            }

            Message::LinkPress(block) => {
                let editable = self.current.as_ref().is_some_and(|n| !n.trashed);
                self.link_menu = None;
                if !editable {
                    return Task::none();
                }
                self.dragging = Some(ImageDrag {
                    block,
                    start: Point::new(self.mouse_x, self.mouse_y),
                    active: false,
                    target: None,
                });
            }

            Message::DragOver(line) => {
                let mut retarget = false;
                let unset = !self.note_line.shown();
                if let Some(d) = &mut self.dragging
                    && d.active
                    && (unset || d.target != Some(line))
                {
                    d.target = Some(line);
                    retarget = true;
                }
                if retarget
                    && let Some(y) = self.note_slot_y(line)
                {
                    self.note_line.to(y, crate::anim::params(self.anim_ms, self.anim_ease10));
                }
            }

            Message::TagPress(entry) => {
                self.tag_drag = Some(TagDrag {
                    entry,
                    start: Point::new(self.mouse_x, self.mouse_y),
                    active: false,
                    target: None,
                });
            }

            Message::TagDragOver(slot) => {
                let mut retarget = false;
                let unset = !self.tag_line.shown();
                if let Some(d) = &mut self.tag_drag
                    && d.active
                    && (unset || d.target != Some(slot))
                {
                    d.target = Some(slot);
                    retarget = true;
                }
                if retarget
                    && let Some(y) = self.tag_slot_y(slot)
                {
                    self.tag_line.to(y, crate::anim::params(self.anim_ms, self.anim_ease10));
                }
            }

            Message::SpaceHint(on) => self.space_hint = on,

            Message::SetWeight(weight) => {
                self.editor_weight = weight;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_editor_weight(handler, weight)
                {
                    tracing::warn!(%why, "saving editor weight");
                }
            }

            Message::AnimStep(delta) => {
                let ms = i32::from(self.anim_ms)
                    + i32::from(delta) * i32::from(crate::anim::STEP_MS);
                self.anim_ms = u16::try_from(ms.clamp(0, i32::from(crate::anim::MAX_MS)))
                    .unwrap_or(crate::anim::DEFAULT_MS);
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self
                        .config
                        .set_animation(handler, self.anim_ms.to_string())
                {
                    tracing::warn!(%why, "saving animation speed");
                }
            }

            Message::AnimEaseStep(delta) => {
                let e = i32::from(self.anim_ease10)
                    + i32::from(delta) * i32::from(crate::anim::STEP_EASE10);
                self.anim_ease10 = u16::try_from(e.clamp(
                    i32::from(crate::anim::MIN_EASE10),
                    i32::from(crate::anim::MAX_EASE10),
                ))
                .unwrap_or(crate::anim::DEFAULT_EASE10);
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self
                        .config
                        .set_animation_ease(handler, self.anim_ease10.to_string())
                {
                    tracing::warn!(%why, "saving animation landing");
                }
            }

            Message::AnimTick(now) => {
                let dt = self
                    .anim_last
                    .replace(now)
                    .map_or(0.016, |t| now.duration_since(t).as_secs_f32());
                let tau = crate::anim::params(self.anim_ms, self.anim_ease10);
                let tags = self.tag_line.step(dt, tau);
                let note = self.note_line.step(dt, tau);
                if !tags && !note {
                    self.anim_last = None;
                }
            }

            Message::AddSpace => {
                let mut order = self.root_entries();
                order.push(String::new());
                self.save_tag_order(order);
            }

            Message::RemoveSpace(entry) => {
                let mut order = self.root_entries();
                if order.get(entry).is_some_and(String::is_empty) {
                    order.remove(entry);
                    self.save_tag_order(order);
                }
            }

            Message::MouseMoved(position) => {
                self.mouse_x = position.x;
                self.mouse_y = position.y;
                if let Some(d) = &mut self.dragging
                    && !d.active
                    && position.distance(d.start) > DRAG_THRESHOLD
                {
                    d.active = true;
                }
                if let Some(d) = &mut self.tag_drag
                    && !d.active
                    && position.distance(d.start) > DRAG_THRESHOLD
                {
                    d.active = true;
                }
                if let Some(rz) = &self.resizing {
                    let w = (rz.start_w as f32 + (position.x - rz.start_x))
                        .clamp(images::MIN_WIDTH as f32, images::MAX_WIDTH as f32)
                        as u32;
                    self.live_width = Some((rz.block, w));
                }
                if let Some(tr) = self.table_resize {
                    let delta = if tr.col {
                        position.x - tr.start
                    } else {
                        position.y - tr.start
                    };
                    if let Some(t) = self.blocks.table_mut(tr.block) {
                        if tr.col {
                            if let Some(w) = t.widths.get_mut(tr.index) {
                                *w = (tr.size + delta).clamp(48.0, 600.0);
                            }
                        } else if let Some(h) = t.heights.get_mut(tr.index) {
                            *h = (tr.size + delta).clamp(26.0, 300.0);
                        }
                    }
                }
            }

            Message::ScrollHover(area, enter) => {
                if enter {
                    self.scroll_hover = Some(area);
                    self.scroll_hover_release = false;
                } else if self.mouse_down {
                    // Mid-drag on the scroller the bar must not vanish under
                    // the pointer; let go of it on release instead.
                    self.scroll_hover_release = true;
                } else if self.scroll_hover == Some(area) {
                    self.scroll_hover = None;
                }
            }

            Message::MousePressed => self.mouse_down = true,

            Message::TableEdit(block, row, col) => {
                self.commit_table_edit();
                self.table_sel = None;
                self.blur_editors();
                if self.blocks.table_mut(block).is_some() {
                    self.table_draft = self
                        .blocks
                        .table_mut(block)
                        .map(|t| t.cell(row, col).to_owned())
                        .unwrap_or_default();
                    self.table_edit = Some((block, row, col));
                    self.table_touched = false;
                    return widget::text_input::focus(self.table_input.clone());
                }
            }

            Message::TableDraft(text) => {
                self.table_draft = text;
                self.table_touched = true;
            }

            Message::TableCommit(mv) => {
                let edited = self.table_edit;
                self.commit_table_edit();
                if let (
                    Some((block, row, col)),
                    TableMove::Down | TableMove::Right | TableMove::Left,
                ) = (edited, mv)
                {
                    let (rows, cols) = self
                        .blocks
                        .table_mut(block)
                        .map(|t| (t.rows.len(), t.cols()))
                        .unwrap_or_default();
                    let next = match mv {
                        TableMove::Down if row + 1 < rows => Some((row + 1, col)),
                        // Enter on the bottom row grows the table, Keynote
                        // style — the next row is always one Enter away.
                        TableMove::Down => {
                            self.record(EditKind::Other);
                            if let Some(t) = self.blocks.table_mut(block) {
                                t.add_row();
                                self.dirty = true;
                                self.last_edit = Instant::now();
                            }
                            Some((row + 1, col))
                        }
                        TableMove::Right if col + 1 < cols => Some((row, col + 1)),
                        TableMove::Right if row + 1 < rows => Some((row + 1, 0)),
                        // Sideways at the edge stays put — arrows must
                        // never dump the caret out of the table.
                        TableMove::Right => Some((row, col)),
                        TableMove::Left if col > 0 => Some((row, col - 1)),
                        TableMove::Left => Some((row, col)),
                        _ => None,
                    };
                    if let Some((r, c)) = next {
                        return self.update(Message::TableEdit(block, r, c));
                    }
                    // Leaving the table: give the keyboard back to the
                    // editor so arrows walk text, not the notes list.
                    return self.focus_editor();
                }
            }

            Message::TableAddRow(block) => {
                self.record(EditKind::Other);
                if let Some(t) = self.blocks.table_mut(block) {
                    t.add_row();
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            }

            Message::TableAddCol(block) => {
                self.record(EditKind::Other);
                if let Some(t) = self.blocks.table_mut(block) {
                    t.add_col();
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            }

            Message::TableRemoveRow(block) => {
                let row = match self.table_edit {
                    Some((b, r, _)) if b == block => r,
                    _ => self.blocks.table_mut(block).map_or(0, |t| t.rows.len() - 1),
                };
                self.table_edit = None;
                self.record(EditKind::Other);
                if let Some(t) = self.blocks.table_mut(block) {
                    t.remove_row(row.max(1));
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            }

            Message::TableRemoveCol(block) => {
                let col = match self.table_edit {
                    Some((b, _, c)) if b == block => c,
                    _ => self.blocks.table_mut(block).map_or(0, |t| t.cols() - 1),
                };
                self.table_edit = None;
                self.record(EditKind::Other);
                if let Some(t) = self.blocks.table_mut(block) {
                    t.remove_col(col);
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            }

            Message::TableResizeStart(block, col, index, size) => {
                self.commit_table_edit();
                self.record(EditKind::Other);
                self.table_resize = Some(TableResize {
                    block,
                    col,
                    index,
                    start: if col { self.mouse_x } else { self.mouse_y },
                    size,
                });
            }

            Message::TableMoney => {
                if self.table_edit.is_some() {
                    self.table_draft = crate::table::cycle_money(&self.table_draft);
                    self.table_touched = true;
                    return widget::text_input::focus(self.table_input.clone());
                }
            }

            Message::TableSelStart(block, row, col) => {
                self.commit_table_edit();
                self.blur_editors();
                self.table_sel = Some(TableSel {
                    block,
                    anchor: (row, col),
                    current: (row, col),
                    active: false,
                });
            }

            Message::TableSelOver(block, row, col) => {
                if let Some(sel) = &mut self.table_sel
                    && sel.block == block
                {
                    sel.current = (row, col);
                    sel.active |= (row, col) != sel.anchor;
                }
            }

            Message::TableFillStart(block, row, col) => {
                self.commit_table_edit();
                self.record(EditKind::Other);
                self.table_fill = Some(TableFill {
                    block,
                    from: (row, col),
                    current: (row, col),
                });
            }

            Message::TableFillOver(block, row, col) => {
                if let Some(f) = &mut self.table_fill
                    && f.block == block
                {
                    f.current = (row, col);
                }
            }

            Message::WindowResized(w, h) => {
                // A maximised size is the compositor's, not a choice worth
                // restoring; remember the free-floating one.
                if !self.core.window.is_maximized && w >= 1.0 && h >= 1.0 {
                    self.win_size = Some((w.round() as u32, h.round() as u32));
                }
            }

            Message::SetTableToolbar(on) => {
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_table_toolbar_off(handler, !on)
                {
                    tracing::warn!(%why, "could not persist table toolbar");
                }
            }

            Message::TablePick(block, row, col) => {
                let base = self.table_draft.clone();
                self.table_draft = format!("{base}{}", cell_ref(row, col));
                self.table_pick = Some(TablePick {
                    block,
                    anchor: (row, col),
                    current: (row, col),
                    base,
                });
                return widget::text_input::focus(self.table_input.clone());
            }

            Message::TablePickOver(block, row, col) => {
                if let Some(pick) = &mut self.table_pick
                    && pick.block == block
                {
                    pick.current = (row, col);
                    let a = cell_ref(pick.anchor.0, pick.anchor.1);
                    self.table_draft = if (row, col) == pick.anchor {
                        format!("{}{a}", pick.base)
                    } else {
                        format!("{}{a}:{}", pick.base, cell_ref(row, col))
                    };
                }
            }

            Message::Quit => {
                // Write anything pending first; closing the main window then
                // takes the normal exit path (which flushes again, harmlessly).
                self.close_current();
                if let Some(id) = self.core.main_window_id() {
                    return cosmic::iced::runtime::window::close(id);
                }
                std::process::exit(0);
            }

            Message::MouseReleased => {
                self.mouse_down = false;
                self.tag_line.clear();
                self.note_line.clear();
                if self.scroll_hover_release {
                    self.scroll_hover_release = false;
                    self.scroll_hover = None;
                }
                if let Some(rz) = self.resizing.take()
                    && let Some((_, w)) = self.live_width.take()
                {
                    self.edit_ref(rz.block, |r| r.width = Some(w));
                }
                if self.table_resize.take().is_some() {
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
                if self.table_pick.take().is_some() {
                    return Task::batch([
                        widget::text_input::focus(self.table_input.clone()),
                        widget::text_input::move_cursor_to_end(self.table_input.clone()),
                    ]);
                }
                if let Some(sel) = self.table_sel {
                    if !sel.active {
                        self.table_sel = None;
                        return self.update(Message::TableEdit(
                            sel.block,
                            sel.anchor.0,
                            sel.anchor.1,
                        ));
                    }
                }
                if let Some(f) = self.table_fill.take() {
                    if let Some((rows, cols)) = fill_targets(f) {
                        if let Some(t) = self.blocks.table_mut(f.block) {
                            let source = t.cell(f.from.0, f.from.1).to_owned();
                            for (r, c) in rows.flat_map(|r| cols.clone().map(move |c| (r, c))) {
                                if (r, c) == f.from {
                                    continue;
                                }
                                let dr = r as isize - f.from.0 as isize;
                                let dc = c as isize - f.from.1 as isize;
                                t.set_cell(r, c, crate::table::translate_formula(&source, dr, dc));
                            }
                            self.dirty = true;
                            self.last_edit = Instant::now();
                        }
                    }
                }
                if let Some(d) = self.tag_drag.take()
                    && d.active
                    && let Some(target) = d.target
                {
                    let mut order = self.root_entries();
                    if d.entry < order.len() && target != d.entry && target != d.entry + 1 {
                        let entry = order.remove(d.entry);
                        let at = if target > d.entry { target - 1 } else { target };
                        order.insert(at.min(order.len()), entry);
                        self.save_tag_order(order);
                    }
                }
                if let Some(d) = self.dragging.take() {
                    if !d.active {
                        // A plain click: pictures open their menu, link
                        // cards wait for a double-click.
                        if matches!(self.blocks.items.get(d.block), Some(Block::Image(_))) {
                            return self.update(Message::ImageMenu(Some(d.block)));
                        }
                        return Task::none();
                    }
                    if let Some(target) = d.target {
                        let snap = self.snapshot();
                        if self.blocks.move_image(d.block, target).is_some() {
                            self.push_undo(snap);
                            self.dirty = true;
                            self.last_edit = Instant::now();
                            return self.focus_editor();
                        }
                    }
                }
            }

            Message::OpenImage(rel) => {
                if let Some(store) = &self.store {
                    let path = images::resolve(store.notes_dir(), &rel);
                    if let Err(err) = open::that_detached(&path) {
                        tracing::warn!(%err, "opening image");
                    }
                }
            }

            Message::FontLoaded => {}

            Message::DesktopSettings(settings) => {
                // `apply` pushes them into libcosmic; the redraw that
                // follows this message picks them up.
                crate::desktop::apply(settings);
            }

            Message::SoftenOutline => {
                tracing::debug!("redraw found libcosmic's window outline");
                if let Some(theme) = crate::desktop::softened_theme() {
                    return cosmic::command::set_theme(theme);
                }
            }

            Message::LoadFonts => {
                crate::desktop::reassert();
                return load_fonts();
            }

            Message::OpenPaths(paths) => {
                let mut opened = None;
                for raw in paths {
                    let path = match images::path_from_arg(&raw) {
                        Some(p) => p,
                        None => {
                            tracing::warn!(arg = raw, "not a file path");
                            continue;
                        }
                    };
                    let Some(store) = self.store.as_mut() else {
                        break;
                    };
                    match store.find_by_path(&path) {
                        Ok(Some(id)) => {
                            tracing::info!(path = %path.display(), "opening");
                            opened = Some(id);
                        }
                        Ok(None) => match store.import(&path) {
                            Ok(note) => {
                                tracing::info!(from = %path.display(), to = %note.path.display(), "imported");
                                opened = Some(note.id);
                            }
                            Err(err) => {
                                tracing::error!(%err, path = %path.display(), "importing");
                                self.save_error = Some(format!("{err:#}"));
                            }
                        },
                        Err(err) => tracing::error!(%err, path = %path.display(), "looking up"),
                    }
                }
                let Some(id) = opened else {
                    return Task::none();
                };
                // An imported note is new to the list; a note in the trash
                // needs that view to be visible at all.
                self.refresh_tags();
                self.refresh_list();
                if !self.notes.iter().any(|n| n.id == id) {
                    self.query.clear();
                    self.view = View::All;
                    self.refresh_list();
                    if !self.notes.iter().any(|n| n.id == id) {
                        self.view = View::Trash;
                        self.refresh_list();
                    }
                }
                return self.update(Message::Select(id));
            }

            Message::SearchFromOutside(text) => {
                tracing::info!(text, "search handed in from outside");
                if matches!(self.view, View::Trash) {
                    self.view = View::All;
                }
                let search = self.update(Message::Search(text));
                return Task::batch([search, self.update(Message::FocusSearch)]);
            }

            Message::Raise => {
                if let Some(id) = self.core.main_window_id() {
                    return Task::batch([
                        window::minimize(id, false),
                        window::gain_focus(id),
                    ]);
                }
            }

            Message::SetTheme(theme) => {
                self.theme = theme;
                self.buffet_on = false;
                if self.icon_theme.is_none() {
                    self.install_icon();
                }
                if let Some(handler) = &self.config_handler {
                    if let Err(why) = self.config.set_theme(handler, theme.key().to_owned()) {
                        tracing::warn!(%why, "could not persist theme");
                    }
                    if let Err(why) = self.config.set_buffet_on(handler, false) {
                        tracing::warn!(%why, "could not persist buffet_on");
                    }
                }
            }

            Message::SetBuffetHighlight(theme) => {
                // A classic theme is not a highlight; ignore rather than
                // silently serving the previous colour.
                if !theme.is_minimal() {
                    return Task::none();
                }
                self.buffet_highlight = theme;
                self.persist_buffet();
            }

            Message::SetBuffetDark(dark) => {
                self.buffet_dark = dark;
                self.buffet_light_mode = false;
                self.persist_buffet();
            }

            Message::SetBuffetLight(light) => {
                self.buffet_light = light;
                self.buffet_light_mode = true;
                self.persist_buffet();
            }

            Message::SetBuffetInk(ink) => {
                self.buffet_ink = ink;
                self.buffet_light_mode = true;
                self.persist_buffet();
            }

            Message::ToggleNav => {
                self.show_nav = !self.show_nav;
                self.persist_layout();
            }

            Message::ToggleList => {
                self.show_list = !self.show_list;
                self.persist_layout();
            }

            Message::ToggleSolo => {
                let solo = !self.show_nav && !self.show_list;
                self.show_nav = solo;
                self.show_list = solo;
                self.persist_layout();
                if !solo {
                    return self.focus_editor();
                }
            }

            Message::ToggleMarkers => {
                self.show_markers = !self.show_markers;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_show_markers(handler, self.show_markers)
                {
                    tracing::warn!(%why, "could not persist marker setting");
                }
            }

            Message::Editor(block, action) => {
                use text_editor::{Action, Edit, Motion};
                tracing::trace!(
                    block,
                    focused = self.blocks.focused,
                    ?action,
                    "editor action"
                );
                // A widget losing focus emits ClearSelection; only real
                // interaction with a block moves the caret there.
                let claims_focus =
                    !matches!(action, Action::ClearSelection | Action::Scroll { .. });
                if claims_focus {
                    self.blocks.focused = block;
                    // Clicking back into the text puts the table away:
                    // commit the open cell, drop its toolbar and selection.
                    self.commit_table_edit();
                    self.table_sel = None;
                }

                self.image_menu = None;
                let editable = self.current.as_ref().is_some_and(|n| !n.trashed);
                // `[]` expanded to `- [ ] ` a keystroke ago: the space the
                // writer types out of habit is already there, so eat it.
                if claims_focus
                    && let Some((b, line, col)) = self.task_space.take()
                    && b == block
                    && matches!(action, Action::Edit(Edit::Insert(' ')))
                    && self.blocks.text(block).is_some_and(|c| {
                        let cur = c.cursor();
                        cur.selection.is_none()
                            && cur.position.line == line
                            && cur.position.column == col
                    })
                {
                    return Task::none();
                }
                // Edges of a block: step over images, or delete the one above.
                if let Some(content) = self.blocks.text(block) {
                    let cursor = content.cursor();
                    let at_start = cursor.position.line == 0
                        && cursor.position.column == 0
                        && cursor.selection.is_none();
                    let on_last_line = cursor.position.line + 1 >= content.line_count();
                    match &action {
                        Action::Edit(Edit::Backspace) if at_start && editable => {
                            if let Some(prev) = block.checked_sub(1)
                                && matches!(
                                    self.blocks.items.get(prev),
                                    Some(
                                        Block::Image(_)
                                            | Block::Rule(_)
                                            | Block::Link(_)
                                            | Block::Table(_)
                                    )
                                )
                            {
                                self.record(EditKind::Other);
                                self.blocks.remove_block(prev);
                                self.dirty = true;
                                self.last_edit = Instant::now();
                                return self.focus_editor();
                            }
                        }
                        Action::Move(Motion::Up) if cursor.position.line == 0 => {
                            if let Some(prev) = self.blocks.text_before(block) {
                                self.blocks.focused = prev;
                                if let Some(c) = self.blocks.text_mut(prev) {
                                    c.perform(Action::Move(Motion::DocumentEnd));
                                }
                                return self.focus_editor();
                            }
                        }
                        Action::Move(Motion::Down) if on_last_line => {
                            if let Some(next) = self.blocks.text_after(block) {
                                self.blocks.focused = next;
                                if let Some(c) = self.blocks.text_mut(next) {
                                    c.perform(Action::Move(Motion::DocumentStart));
                                }
                                return self.focus_editor();
                            }
                        }
                        _ => {}
                    }
                }
                let is_edit = action.is_edit();
                let is_click = matches!(action, Action::Click(_));
                let finishes_line = matches!(action, Action::Edit(Edit::Enter | Edit::Paste(_)));
                let closes_bracket = matches!(action, Action::Edit(Edit::Insert(']')));
                if is_edit && editable {
                    let kind = match &action {
                        Action::Edit(Edit::Insert(_) | Edit::Enter | Edit::Indent) => {
                            EditKind::Typing
                        }
                        Action::Edit(Edit::Backspace | Edit::Delete | Edit::Unindent) => {
                            EditKind::Deleting
                        }
                        _ => EditKind::Other,
                    };
                    self.record(kind);
                }
                let moves_caret =
                    is_edit || matches!(action, Action::Move(_) | Action::Select(_));
                // Enter on a list line carries the list on (or ends it).
                let listed = editable
                    && matches!(action, Action::Edit(Edit::Enter))
                    && self
                        .blocks
                        .text_mut(block)
                        .is_some_and(crate::blocks::continue_list);
                if !listed && let Some(content) = self.blocks.text_mut(block) {
                    content.perform(action);
                }
                // Typing and caret motion keep the caret in view; a click
                // or drag is already where the pointer is.
                if moves_caret && let Some(content) = self.blocks.text(block)
                {
                    content.request_reveal();
                }
                if is_edit && editable {
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
                // A finished `---` line (Enter / paste) becomes a rule block.
                if finishes_line && editable && self.blocks.needs_resplit() && self.blocks.resplit()
                {
                    return Task::batch([self.queue_previews(), self.focus_editor()]);
                }
                // `[]` at the start of a line becomes a task box.
                if closes_bracket
                    && editable
                    && let Some(content) = self.blocks.text_mut(block)
                    && crate::blocks::expand_task_shorthand(content)
                {
                    // A fresh task box renders at once; no raw `- [ ] ` first.
                    content.render_now();
                    let cur = content.cursor();
                    self.task_space = Some((block, cur.position.line, cur.position.column));
                }
                // Clicking a task box ticks / unticks it.
                if is_click && editable {
                    let before = self.snapshot();
                    let marker = self.task_marker.clone();
                    if let Some(content) = self.blocks.text_mut(block)
                        && crate::blocks::toggle_task_at_cursor(content, &marker)
                    {
                        content.render_now();
                        self.push_undo(before);
                        self.last_undo_kind = EditKind::Other;
                        self.dirty = true;
                        self.last_edit = Instant::now();
                    }
                }
            }

            Message::AutosaveTick => {
                if self.dirty && self.last_edit.elapsed() >= AUTOSAVE_DELAY {
                    self.flush();
                    return self.update_title();
                }
            }

            Message::SetRuleSize(size) => {
                self.rule_size = size;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_rule_size(handler, size.key().to_owned())
                {
                    tracing::warn!(%why, "could not persist rule size");
                }
            }

            Message::SetTagLineSize(size) => {
                self.tag_line_size = size;
                if let Some(handler) = &self.config_handler
                    && let Err(why) =
                        self.config.set_tag_line_size(handler, size.key().to_owned())
                {
                    tracing::warn!(%why, "could not persist tag line size");
                }
            }

            Message::SetCardLine(size) => {
                self.card_line = size;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_card_line(handler, size.key().to_owned())
                {
                    tracing::warn!(%why, "could not persist card line");
                }
            }

            Message::SetCardBorder(on) => {
                self.card_border = on;
                if let Some(handler) = &self.config_handler
                    && let Err(why) = self.config.set_card_border_off(handler, !on)
                {
                    tracing::warn!(%why, "could not persist card border");
                }
            }

            Message::WindowSizeTick => {
                if let Some((w, h)) = self.win_size
                    && let Some(handler) = &self.config_handler
                {
                    for why in [
                        self.config.set_window_width(handler, w),
                        self.config.set_window_height(handler, h),
                    ]
                    .into_iter()
                    .filter_map(Result::err)
                    {
                        tracing::warn!(%why, "could not persist window size");
                    }
                }
            }

            Message::SetView(view) => {
                if self.view == view {
                    return Task::none();
                }
                self.close_current();
                self.view = view;
                self.refresh_list();
                if let Some(first) = self.notes.first().map(|n| n.id.clone()) {
                    self.open_note(&first);
                }
                return self.update_title();
            }

            Message::NavigateNotes(delta) => {
                // Inside a table, ↑/↓ walk its cells — the single-line cell
                // input ignores them, so they land here, and hopping notes
                // mid-table would be a shock.
                if let Some((block, row, col)) = self.table_edit {
                    let rows = self.blocks.table_mut(block).map_or(0, |t| t.rows.len());
                    let next = if delta > 0 {
                        row + 1
                    } else {
                        row.wrapping_sub(1)
                    };
                    if next < rows {
                        return self.update(Message::TableEdit(block, next, col));
                    }
                    return Task::none();
                }
                let len = self.notes.len();
                if len == 0 {
                    return Task::none();
                }
                let at = self
                    .current
                    .as_ref()
                    .and_then(|c| self.notes.iter().position(|n| n.id == c.id));
                let next = match at {
                    Some(i) => (i as i64 + i64::from(delta)).clamp(0, len as i64 - 1) as usize,
                    None if delta > 0 => 0,
                    None => len - 1,
                };
                let id = self.notes[next].id.clone();
                // Keep the selection in view (rows vary in height; this is close enough).
                let y = if len > 1 {
                    next as f32 / (len - 1) as f32
                } else {
                    0.0
                };
                let scroll = cosmic::iced::widget::scrollable::snap_to(
                    self.notes_scroll_id.clone(),
                    cosmic::iced::widget::scrollable::RelativeOffset {
                        x: None,
                        y: Some(y),
                    },
                );
                return Task::batch([self.update(Message::Select(id)), scroll]);
            }

            Message::Select(id) => {
                if self.current.as_ref().is_some_and(|n| n.id == id) {
                    return Task::none();
                }
                self.close_current();
                self.open_note(&id);
                return Task::batch([self.queue_previews(), self.update_title()]);
            }

            Message::NewNote => {
                self.dock_open = false;
                if matches!(self.view, View::Trash) {
                    self.view = View::All;
                }
                self.query.clear();
                // Every note opens as a heading; a note started inside a
                // folder also carries that tag from the outset.
                let body = match &self.view {
                    View::Tag(t) => format!("# \n\n#{t}\n"),
                    _ => "# ".to_owned(),
                };
                return self.create_note(body, false);
            }

            Message::TrashCurrent => {
                self.flush();
                if let (Some(store), Some(note)) = (self.store.as_mut(), self.current.take())
                    && let Err(err) = store.trash(&note.id)
                {
                    tracing::error!(%err, "trashing note");
                }
                self.after_store_change();
                return self.update_title();
            }

            Message::RestoreCurrent => {
                if let (Some(store), Some(note)) = (self.store.as_mut(), self.current.take())
                    && let Err(err) = store.restore(&note.id)
                {
                    tracing::error!(%err, "restoring note");
                }
                self.after_store_change();
                return self.update_title();
            }

            Message::DeleteCurrentForever => {
                if let (Some(store), Some(note)) = (self.store.as_mut(), self.current.take())
                    && let Err(err) = store.delete_forever(&note.id)
                {
                    tracing::error!(%err, "deleting note");
                }
                self.after_store_change();
                return self.update_title();
            }

            Message::EmptyTrash => {
                self.current = None;
                if let Some(store) = self.store.as_mut()
                    && let Err(err) = store.empty_trash()
                {
                    tracing::error!(%err, "emptying trash");
                }
                self.after_store_change();
                return self.update_title();
            }

            Message::TogglePin => {
                self.flush();
                let target = self.current.as_ref().map(|n| (n.id.clone(), !n.pinned));
                if let (Some(store), Some((id, pinned))) = (self.store.as_mut(), target) {
                    match store.set_pinned(&id, pinned) {
                        Ok(Some(note)) => {
                            if let Some(current) = self.current.as_mut() {
                                current.pinned = note.pinned;
                                current.modified = note.modified;
                            }
                        }
                        Ok(None) => {}
                        Err(err) => tracing::error!(%err, "pinning note"),
                    }
                }
                self.sync_soon();
                self.refresh_list();
            }

            Message::Search(query) => {
                if query.trim().eq_ignore_ascii_case("coffee") && !self.coffee_unlocked {
                    // The counter: the hidden theme appears in Options → Colour.
                    self.coffee_unlocked = true;
                    if let Some(handler) = &self.config_handler
                        && let Err(why) = self.config.set_coffee_unlocked(handler, true)
                    {
                        tracing::warn!(%why, "saving the coffee secret");
                    }
                }
                self.query = query;
                self.refresh_list();
            }

            Message::FocusSearch => {
                return widget::text_input::focus(self.search_id.clone());
            }

            Message::Format(format) => {
                if self.current.as_ref().is_some_and(|n| !n.trashed) {
                    self.apply_format(format, false);
                    return self.focus_editor();
                }
            }

            Message::MarkSweep => {
                if self.current.as_ref().is_some_and(|n| !n.trashed) {
                    self.apply_format(Format::Mark, true);
                    return self.focus_editor();
                }
            }

            Message::ToggleDock => {
                self.dock_open = !self.dock_open;
                if self.dock_open {
                    return widget::text_input::focus(self.folder_id.clone());
                }
            }

            Message::NewFolderName(name) => {
                self.new_folder = name;
            }

            Message::CreateFolder => {
                let name = std::mem::take(&mut self.new_folder);
                let created = self.store.as_mut().and_then(|s| match s.add_folder(&name) {
                    Ok(tag) => tag,
                    Err(err) => {
                        tracing::error!(%err, "creating folder");
                        None
                    }
                });
                self.dock_open = false;
                if let Some(tag) = created {
                    self.refresh_tags();
                    return self.update(Message::SetView(View::Tag(tag)));
                }
            }

            Message::ScriptTick => {
                let step = self.script.as_mut().and_then(debug_script::Runner::next);
                if let Some(step) = step {
                    tracing::debug!(?step, "JJB_SCRIPT step");
                    return self.run_step(step);
                }
            }

            Message::TakeScreenshot => {
                if self.screenshot_pending {
                    self.screenshot_pending = false;
                    if let Some(id) = self.core.main_window_id() {
                        return cosmic::iced::window::screenshot(id).map(|shot| {
                            cosmic::Action::App(Message::ScreenshotTaken(Arc::new(shot)))
                        });
                    }
                }
            }

            Message::ScreenshotTaken(shot) => {
                if let Some(path) = std::env::var_os("JJB_SCREENSHOT") {
                    match image::save_buffer(
                        &path,
                        &shot.rgba,
                        shot.size.width,
                        shot.size.height,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(()) => {
                            tracing::info!(path = %path.to_string_lossy(), "screenshot saved")
                        }
                        Err(err) => tracing::error!(%err, "saving screenshot"),
                    }
                }
                self.close_current();
                std::process::exit(0);
            }

            Message::Key(modifiers, key, physical) => {
                // A table cell is open: Tab walks the row, Escape drops the
                // draft. (Enter commits through the input's on_submit.)
                if self.table_edit.is_some() {
                    match key {
                        keyboard::Key::Named(keyboard::key::Named::Tab) => {
                            return self.update(Message::TableCommit(if modifiers.shift() {
                                TableMove::Left
                            } else {
                                TableMove::Right
                            }));
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
                            if !self.table_touched && modifiers.is_empty() =>
                        {
                            return self.update(Message::TableCommit(TableMove::Left));
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowRight)
                            if !self.table_touched && modifiers.is_empty() =>
                        {
                            return self.update(Message::TableCommit(TableMove::Right));
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.table_edit = None;
                            self.table_draft.clear();
                            return self.focus_editor();
                        }
                        _ => {}
                    }
                }
                if let Some(sel) = self.table_sel
                    && sel.active
                    && self.table_edit.is_none()
                {
                    match key {
                        keyboard::Key::Named(
                            keyboard::key::Named::Delete | keyboard::key::Named::Backspace,
                        ) => {
                            self.record(EditKind::Other);
                            if let Some(t) = self.blocks.table_mut(sel.block) {
                                let (r0, r1) = (
                                    sel.anchor.0.min(sel.current.0),
                                    sel.anchor.0.max(sel.current.0),
                                );
                                let (c0, c1) = (
                                    sel.anchor.1.min(sel.current.1),
                                    sel.anchor.1.max(sel.current.1),
                                );
                                for r in r0..=r1 {
                                    for c in c0..=c1 {
                                        t.set_cell(r, c, String::new());
                                    }
                                }
                                self.dirty = true;
                                self.last_edit = Instant::now();
                            }
                            return Task::none();
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.table_sel = None;
                            return Task::none();
                        }
                        _ => {}
                    }
                }
                let enter = matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter));
                if enter && modifiers.control() && modifiers.shift() {
                    return self.update(Message::ToggleCoffee);
                }
                if self.coffee.is_some() {
                    self.coffee = None;
                    return Task::none();
                }
                if (self.dragging.is_some() || self.tag_drag.is_some())
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
                {
                    self.dragging = None;
                    self.tag_drag = None;
                    self.tag_line.clear();
                    self.note_line.clear();
                    return Task::none();
                }
                let action = self
                    .key_binds
                    .iter()
                    .find(|(bind, _)| bind.matches(modifiers, &key, Some(&physical)))
                    .map(|(_, action)| action.message());
                tracing::trace!(?modifiers, ?key, matched = action.is_some(), "key bind lookup");
                if let Some(message) = action {
                    return self.update(message);
                }
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                // A changed notes_dir takes effect on next launch; everything else is live.
                self.theme = retro::Theme::from_key(&config.theme);
                self.buffet_on = config.buffet_on;
                self.buffet_light_mode = config.buffet_mode == "light";
                self.buffet_highlight = buffet_highlight_from_config(&config.buffet_highlight);
                self.buffet_dark = retro::Dark::from_key(&config.buffet_dark);
                self.buffet_light = retro::Light::from_key(&config.buffet_light);
                self.buffet_ink = retro::Ink::from_key(&config.buffet_ink);
                self.show_markers = config.show_markers;
                self.show_nav = !config.hide_nav;
                self.show_list = !config.hide_list;
                self.editor_font = retro::EditorFont::from_key(&config.editor_font);
                self.ui_font = retro::EditorFont::from_key(&config.ui_font);
                self.list_font = if config.list_font.is_empty() {
                    self.ui_font
                } else {
                    retro::EditorFont::from_key(&config.list_font)
                };
                self.title_font = title_font_from_config(&config.title_font);
                self.font_size = size_from_config(
                    config.editor_font_size,
                    retro::FONT_SIZE_DEFAULT,
                    retro::FONT_SIZE_MIN,
                    retro::FONT_SIZE_MAX,
                );
                self.sidebar_size = pane_size_from_config(config.sidebar_font_size);
                self.list_size = pane_size_from_config(config.list_font_size);
                self.dock_size = retro::DockSize::from_key(&config.dock_size);
        self.anim_ms = crate::anim::ms_from_key(&config.animation);
        self.anim_ease10 = crate::anim::ease_from_key(&config.animation_ease);
        self.editor_weight = match config.editor_weight {
            w @ (200 | 300 | 500) => w,
            _ => 400,
        };
                self.task_marker = task_marker_from_config(&config.task_marker);
                self.measure = retro::Measure::from_key(&config.text_width);
                self.rule_size = retro::LineSize::from_key(&config.rule_size);
                self.card_line = retro::LineSize::from_key(&config.card_line);
                self.tag_line_size = retro::LineSize::from_key(&config.tag_line_size);
                self.card_border = !config.card_border_off;
                self.config = config;
                // A theme or buffet change from outside (another instance,
                // a config edit) repaints a follow-mode icon too.
                if self.icon_theme.is_none() {
                    self.install_icon();
                }
            }

            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    tracing::warn!(%err, url, "failed to open url");
                }
            },
        }
        Task::none()
    }

    /// The themes on offer: the sixteen classics and the minimal eleven,
    /// plus Long Black once it is found.
    fn themes(&self) -> Vec<retro::Theme> {
        let mut all = retro::Theme::ALL.to_vec();
        if self.coffee_unlocked || self.theme == retro::Theme::LongBlack {
            all.push(retro::Theme::LongBlack);
        }
        all
    }

    /// The neon coffee sign: a braille-dot cup whose steam drifts frame by
    /// frame, tubes buzzing now and then, on a near-black night.
    fn coffee_overlay<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let (frame, glow) = self.coffee.unwrap_or((0, 1.0));
        let art = crate::coffee::FRAMES[frame % crate::coffee::FRAMES.len()];
        let mut lines = art.lines();
        let steam: String = lines.by_ref().take(4).collect::<Vec<_>>().join("\n");
        let cup: String = lines.collect::<Vec<_>>().join("\n");
        let pink = cosmic::iced::Color::from_rgb8(0xff, 0x4f, 0xa3).scale_alpha(glow);
        let cyan = cosmic::iced::Color::from_rgb8(0x4f, 0xe3, 0xff).scale_alpha(glow);
        let size = 34.0;
        // Neon: a soft halo (bigger, faint) under a crisp tube.
        let neon = |text: String, color: cosmic::iced::Color| {
            let halo = widget::text(text.clone())
                .font(retro::mono())
                .size(size + 1.5)
                .class(cosmic::theme::Text::Color(color.scale_alpha(0.28)));
            let tube = widget::text(text)
                .font(retro::mono())
                .size(size)
                .class(cosmic::theme::Text::Color(color));
            cosmic::iced::widget::stack([
                widget::container(halo).padding([0, 0, 0, 1]).into(),
                tube.into(),
            ])
        };
        let sign = widget::column::with_capacity(4)
            .push(neon(steam, cyan))
            .push(neon(cup, pink))
            .push(
                widget::container(
                    widget::text(fl!("coffee-caption"))
                        .font(retro::TITLE_FONT)
                        .size(64)
                        .class(cosmic::theme::Text::Color(pink)),
                )
                .padding([18, 0, 0, 0]),
            )
            .push(
                widget::text(fl!("coffee-hint"))
                    .font(retro::TITLE_FONT)
                    .size(22)
                    .class(cosmic::theme::Text::Color(cyan.scale_alpha(0.8))),
            )
            .align_x(Alignment::Center)
            .spacing(4);
        let night = cosmic::iced::Color::from_rgb8(0x07, 0x05, 0x0d).scale_alpha(0.95);
        let _ = p;
        widget::mouse_area(
            widget::container(sign)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .class(cosmic::theme::Container::custom(move |_| {
                    widget::container::Style {
                        background: Some(cosmic::iced::Background::Color(night)),
                        ..Default::default()
                    }
                })),
        )
        .on_press(Message::ToggleCoffee)
        .into()
    }

    /// Write the chosen (or theme-following) icon into the user's icon theme.
    fn install_icon(&self) {
        // Following the colour theme follows the buffet too when it is on.
        let palette = match self.icon_theme {
            Some(theme) => theme.palette(self.core.system_theme()),
            None => self.palette(),
        };
        let svg = crate::icon::svg(&palette);
        // Off the UI thread: a changed icon rewrites hicolor files and runs
        // gtk-update-icon-cache, too slow for the buffet's live tasting.
        std::thread::spawn(move || match crate::icon::install(Self::APP_ID, &svg) {
            Ok(true) => tracing::info!("launcher icon updated"),
            Ok(false) => {}
            Err(err) => tracing::warn!(%err, "installing launcher icon"),
        });
    }

    /// The face pane titles are set in: VT323 as-is, anything else bold.
    fn title_font(&self) -> cosmic::font::Font {
        let f = self.title_font.font();
        if self.title_font.has_bold() {
            cosmic::font::Font {
                weight: cosmic::iced::font::Weight::Bold,
                ..f
            }
        } else {
            f
        }
    }

    fn ui_font(&self) -> cosmic::font::Font {
        self.ui_font.font()
    }

    fn list_font(&self) -> cosmic::font::Font {
        self.list_font.font()
    }

    fn apply_pairing(&mut self, pair: &retro::Pairing) {
        self.editor_font = pair.body;
        self.ui_font = pair.ui;
        self.list_font = pair.ui;
        self.title_font = pair.title;
        if let Some(handler) = &self.config_handler {
            for (why, _) in [
                self.config
                    .set_editor_font(handler, pair.body.key().to_owned())
                    .err(),
                self.config
                    .set_ui_font(handler, pair.ui.key().to_owned())
                    .err(),
                self.config
                    .set_title_font(handler, pair.title.key().to_owned())
                    .err(),
            ]
            .into_iter()
            .flatten()
            .map(|e| (e, ()))
            {
                tracing::warn!(%why, "saving font pairing");
            }
        }
        if let Some(handler) = &self.config_handler
            && let Err(why) = self
                .config
                .set_list_font(handler, pair.ui.key().to_owned())
        {
            tracing::warn!(%why, "saving list font");
        }
    }

    fn palette(&self) -> Palette {
        if self.buffet_on {
            if self.buffet_light_mode {
                retro::buffet_light(self.buffet_accent(), self.buffet_light, self.buffet_ink)
            } else {
                retro::buffet_dark(self.buffet_accent(), self.buffet_dark)
            }
        } else {
            self.theme.palette(self.core.system_theme())
        }
    }

    /// What identifies the palette themed images are processed with: the
    /// theme key, or the buffet pairing while it is serving.
    fn palette_id(&self) -> String {
        if self.buffet_on {
            if self.buffet_light_mode {
                format!(
                    "buffet:{}/{}/{}",
                    self.buffet_highlight.key(),
                    self.buffet_light.key(),
                    self.buffet_ink.key()
                )
            } else {
                format!(
                    "buffet:{}/{}",
                    self.buffet_highlight.key(),
                    self.buffet_dark.key()
                )
            }
        } else {
            self.theme.key().to_owned()
        }
    }

    /// The buffet's highlight colour (the stored minimal theme's accent).
    fn buffet_accent(&self) -> cosmic::iced::Color {
        self.buffet_highlight
            .minimal_accent()
            .unwrap_or_else(|| retro::Theme::Tomato.minimal_accent().unwrap())
    }

    /// Switch the buffet on and write its three config entries.
    fn persist_buffet(&mut self) {
        self.buffet_on = true;
        if self.icon_theme.is_none() {
            self.install_icon();
        }
        if let Some(handler) = &self.config_handler {
            let mode = if self.buffet_light_mode { "light" } else { "dark" };
            let writes = [
                self.config.set_buffet_on(handler, true),
                self.config.set_buffet_mode(handler, mode.to_owned()),
                self.config
                    .set_buffet_highlight(handler, self.buffet_highlight.key().to_owned()),
                self.config
                    .set_buffet_dark(handler, self.buffet_dark.key().to_owned()),
                self.config
                    .set_buffet_light(handler, self.buffet_light.key().to_owned()),
                self.config
                    .set_buffet_ink(handler, self.buffet_ink.key().to_owned()),
            ];
            for why in writes.into_iter().filter_map(Result::err) {
                tracing::warn!(%why, "could not persist buffet");
            }
        }
    }

    // ----- frames -----

    fn views_frame<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let rows = [
            (View::All, fl!("nav-all-notes"), self.view_counts[0]),
            (View::Untagged, fl!("nav-untagged"), self.view_counts[1]),
            (View::Trash, fl!("nav-trash"), self.view_counts[2]),
        ];
        let sz = f32::from(self.sidebar_size);
        let mut col = widget::column::with_capacity(3).spacing(2);
        for (view, label, count) in rows {
            let selected = self.view == view;
            let marker = if selected { "▌" } else { " " };
            let ui = self.ui_font();
            let row = widget::row::with_capacity(3)
                .push(retro::accent(p, marker).size(sz))
                .push(retro::text(p, label).font(ui).size(sz).width(Length::Fill))
                .push(retro::dim(p, count.to_string()).font(ui).size(sz - 1.0))
                .spacing(8)
                .align_y(Alignment::Center);
            col = col.push(
                widget::button::custom(row)
                    .padding([4, 8])
                    .width(Length::Fill)
                    .class(retro::row_class(p, selected))
                    .on_press(Message::SetView(view)),
            );
        }
        // The settings gear lives in the sidebar's top-right corner.
        let open = self.core.window.show_context && self.context_page == ContextPage::Themes;
        let gear: Element<'a, Message> = widget::tooltip(
            widget::button::custom(retro::accent(p, "⚙").size(17))
                .padding([0, 6])
                .class(retro::row_class(p, open))
                .on_press(Message::ToggleContextPage(ContextPage::Themes)),
            retro::dim(
                p,
                self.with_shortcut(fl!("theme-colours"), MenuAction::Themes),
            ),
            widget::tooltip::Position::Bottom,
        )
        .into();
        retro::pane_el(
            p,
            self.title_font(),
            fl!("frame-views"),
            Some(gear),
            col,
            p.bg,
        )
    }

    /// The sidebar's top-level entries, top to bottom: root tag names plus
    /// "" spacer lines. Saved order first (dropping tags that no longer
    /// exist), any new tags slotted in alphabetically.
    fn root_entries(&self) -> Vec<String> {
        let roots: Vec<&str> = self
            .tags
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| !n.contains('/'))
            .collect();
        let mut out: Vec<String> = self
            .config
            .tag_order
            .iter()
            .filter(|e| e.is_empty() || roots.contains(&e.as_str()))
            .cloned()
            .collect();
        for root in roots {
            if !out.iter().any(|e| e == root) {
                let at = out
                    .iter()
                    .position(|e| !e.is_empty() && e.as_str() > root)
                    .unwrap_or(out.len());
                out.insert(at, root.to_owned());
            }
        }
        out
    }

    fn save_tag_order(&mut self, order: Vec<String>) {
        if let Some(handler) = &self.config_handler
            && let Err(why) = self.config.set_tag_order(handler, order)
        {
            tracing::warn!(%why, "saving tag order");
        }
    }

    /// Vertical position (within the tag list) of the gap before root
    /// entry `slot`, read from the probes recorded at the last layout.
    fn tag_slot_y(&self, slot: usize) -> Option<f32> {
        let rows = self.tag_rows.borrow();
        let area = self.tag_area.get();
        let y = if let Some(c) = rows.get(slot) {
            c.get().y
        } else {
            let r = rows.last()?.get();
            r.y + r.height
        };
        Some((y - area.y - 2.0).max(0.0))
    }

    /// Vertical position (within the note body) of the gap before body
    /// line `line`, from the drag rows' probes.
    fn note_slot_y(&self, line: usize) -> Option<f32> {
        let slots = self.note_slots.borrow();
        let area = self.note_area.get();
        let y = match slots.get(&line).map(|c| c.get()).filter(|r| r.height > 0.0) {
            Some(r) => r.y,
            None => {
                // Past the end, or a line between blocks: sit under the
                // nearest measured line above it.
                let r = slots
                    .iter()
                    .filter(|(k, c)| **k < line && c.get().height > 0.0)
                    .max_by_key(|(k, _)| **k)?
                    .1
                    .get();
                r.y + r.height
            }
        };
        Some((y - area.y - 2.0).max(0.0))
    }

    /// The probe cell recording where body line `line` sits during a drag.
    fn note_cell(&self, line: usize) -> std::rc::Rc<std::cell::Cell<cosmic::iced::Rectangle>> {
        self.note_slots.borrow_mut().entry(line).or_default().clone()
    }

    /// The editor body face at the chosen weight. Markdown bold and
    /// headings still come out at full Bold on top of it.
    fn body_font(&self) -> cosmic::font::Font {
        use cosmic::iced::font::Weight;
        cosmic::font::Font {
            weight: match self.editor_weight {
                200 => Weight::ExtraLight,
                300 => Weight::Light,
                500 => Weight::Medium,
                _ => Weight::Normal,
            },
            ..self.editor_font.font()
        }
    }

    fn tags_frame<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let sz = f32::from(self.sidebar_size);
        let entries = self.root_entries();
        let drag = self.tag_drag.filter(|d| d.active);
        // During a drag every row splits into a top half (drop above) and a
        // bottom half (drop below); `slot` reports the gap before `entry`.
        let slot = |entry: usize| {
            widget::mouse_area(
                widget::Space::new()
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_move(move |_| Message::TagDragOver(entry))
            .interaction(mouse::Interaction::Grabbing)
        };
        let halves = |k: usize| {
            widget::column::with_capacity(2)
                .push(slot(k))
                .push(slot(k + 1))
                .width(Length::Fill)
                .height(Length::Fill)
        };
        self.tag_rows.borrow_mut().resize_with(entries.len(), || {
            std::rc::Rc::new(std::cell::Cell::new(cosmic::iced::Rectangle::default()))
        });
        let mut col =
            widget::column::with_capacity(self.tags.len() + entries.len()).spacing(1);
        for (k, entry) in entries.iter().enumerate() {
            if entry.is_empty() {
                // A spacer line: drag it like a tag; right-click removes it.
                let line = widget::mouse_area(
                    widget::container(retro::spacer_line(p, self.tag_line_size.rule_px()))
                        .width(Length::Fill),
                )
                .on_press(Message::TagPress(k))
                .on_right_press(Message::RemoveSpace(k))
                .interaction(mouse::Interaction::Grab);
                let el: Element<'a, Message> = if drag.is_some() {
                    cosmic::iced::widget::stack([line.into(), halves(k).into()])
                        .width(Length::Fill)
                        .into()
                } else {
                    line.into()
                };
                col = col.push(crate::probe::probe(el, self.tag_rows.borrow()[k].clone()));
                continue;
            }
            let mut ecol = widget::column::with_capacity(4).spacing(1);
            let prefix = format!("{entry}/");
            for (name, count) in self
                .tags
                .iter()
                .filter(|(n, _)| n == entry || n.starts_with(&prefix))
            {
            // Hidden while any ancestor is folded.
            let folded_away = name
                .match_indices('/')
                .any(|(i, _)| self.collapsed.contains(&name[..i]));
            if folded_away {
                continue;
            }
            let depth = name.matches('/').count();
            let leaf = name.rsplit('/').next().unwrap_or(name);
            let has_children = self.tags.iter().any(|(other, _)| {
                other.len() > name.len() + 1 && other.starts_with(&format!("{name}/"))
            });
            let folded = self.collapsed.contains(name);
            let selected = matches!(&self.view, View::Tag(t) if t == name);
            let count_text = if *count > 0 {
                count.to_string()
            } else {
                String::new()
            };
            let chevron: Element<'a, Message> = if has_children {
                widget::button::custom(
                    retro::dim(p, if folded { "▸" } else { "▾" })
                        .size(sz - 1.0)
                        .class(cosmic::theme::Text::Color(p.dim)),
                )
                .padding([0, 3])
                .class(retro::row_class(p, false))
                .on_press(Message::ToggleTagFold(name.clone()))
                .into()
            } else {
                widget::container(retro::dim(p, " ").size(sz - 1.0))
                    .padding([0, 3])
                    .into()
            };
            let row = widget::row::with_capacity(5)
                .push(widget::Space::new().width(Length::Fixed(14.0 * depth as f32)))
                .push(chevron)
                .push(match crate::glyph::for_tag(name, &self.tag_icons) {
                    Some(icon) => Element::from(
                        widget::svg(icon.handle(self.icon_set, p.accent2))
                            .width(sz + 3.0)
                            .height(sz + 3.0),
                    ),
                    None => retro::accent2(p, "#").size(sz).into(),
                })
                .push(
                    retro::text(p, leaf.to_owned())
                        .font(self.ui_font())
                        .size(sz)
                        .width(Length::Fill),
                )
                .push(
                    retro::dim(p, count_text)
                        .font(self.ui_font())
                        .size(sz - 1.0),
                )
                .spacing(4)
                .align_y(Alignment::Center);
            let button = widget::button::custom(row)
                .padding([3, 8, 3, 4])
                .width(Length::Fill)
                .class(retro::row_class(p, selected))
                .on_press(Message::SetView(View::Tag(name.clone())))
                .on_press_down(Message::TagPress(k));
            // Right-click: a small menu (rename); the rename is an inline input.
            let item =
                widget::mouse_area(button).on_right_press(Message::TagMenu(Some(name.clone())));
            let popup: Option<Element<'a, Message>> =
                if let Some((t, draft)) = self.tag_rename.as_ref().filter(|(t, _)| t == name) {
                    let _ = t;
                    Some(
                        widget::container(
                            widget::text_input(fl!("tag-rename-placeholder"), draft)
                                .id(self.rename_id.clone())
                                .font(retro::mono())
                                .size(sz)
                                .padding([3, 8])
                                .width(Length::Fixed(200.0))
                                .leading_icon(
                                    widget::container(retro::accent2(p, "#"))
                                        .padding([0, 0, 0, 8])
                                        .into(),
                                )
                                .style(retro::search_class(p))
                                .on_input(Message::TagRenameInput)
                                .on_submit(|_| Message::TagRenameCommit),
                        )
                        .padding(4)
                        .class(retro::dock_class(p))
                        .into(),
                    )
                } else if self.tag_icon_pick.as_deref() == Some(name.as_str()) {
                    // The 8-bit icon grid for this folder.
                    let current = self.tag_icons.get(name.as_str()).copied();
                    let mut tiles: Vec<Element<'a, Message>> =
                        Vec::with_capacity(crate::glyph::Icon::ALL.len() + 1);
                    for icon in crate::glyph::Icon::ALL {
                        let handle = icon.handle(self.icon_set, p.accent2);
                        tiles.push(
                            widget::tooltip(
                                widget::button::custom(widget::svg(handle).width(22).height(22))
                                    .padding(4)
                                    .class(retro::row_class(p, current == Some(icon)))
                                    .on_press(Message::SetTagIcon(name.clone(), Some(icon))),
                                retro::dim(p, icon.label()),
                                widget::tooltip::Position::Top,
                            )
                            .into(),
                        );
                    }
                    tiles.push(
                        widget::button::custom(retro::dim(p, fl!("tag-icon-none")).size(sz))
                            .padding([6, 8])
                            .class(retro::row_class(p, current.is_none()))
                            .on_press(Message::SetTagIcon(name.clone(), None))
                            .into(),
                    );
                    Some(
                        widget::container(widget::flex_row(tiles).spacing(2))
                            .padding(6)
                            .width(Length::Fixed(232.0))
                            .class(retro::dock_class(p))
                            .into(),
                    )
                } else if self.tag_menu.as_deref() == Some(name.as_str()) {
                    let item = |label: String, msg: Message| {
                        widget::button::custom(retro::text(p, label).size(sz))
                            .padding([3, 10])
                            .width(Length::Fixed(160.0))
                            .class(retro::row_class(p, false))
                            .on_press(msg)
                    };
                    Some(
                        widget::container(
                            widget::column::with_capacity(2)
                                .push(item(
                                    fl!("tag-rename"),
                                    Message::TagRenameStart(name.clone()),
                                ))
                                .push(item(fl!("tag-icon"), Message::TagIconPick(name.clone())))
                                .spacing(1),
                        )
                        .padding(4)
                        .class(retro::dock_class(p))
                        .into(),
                    )
                } else {
                    None
                };
            let base: Element<'a, Message> = match popup {
                Some(popup) => widget::popover(item)
                    .popup(popup)
                    .position(widget::popover::Position::Bottom)
                    .on_close(Message::TagRenameCancel)
                    .into(),
                None => item.into(),
            };
            let el: Element<'a, Message> = if drag.is_some() {
                cosmic::iced::widget::stack([base, halves(k).into()])
                    .width(Length::Fill)
                    .into()
            } else {
                base
            };
            ecol = ecol.push(el);
            }
            col = col.push(crate::probe::probe(
                ecol.width(Length::Fill),
                self.tag_rows.borrow()[k].clone(),
            ));
        }
        let body: Element<'_, Message> = if self.tags.is_empty() {
            widget::container(retro::dim(p, fl!("no-tags-yet")))
                .padding([8, 8])
                .into()
        } else {
            // Below the list: an invisible strip that shows a line on hover;
            // clicking it adds a draggable spacer to the tag list.
            let hint: Element<'a, Message> = if self.space_hint {
                retro::spacer_line(p, self.tag_line_size.rule_px())
            } else {
                widget::Space::new().width(Length::Fill).into()
            };
            let strip = widget::mouse_area(
                widget::container(hint)
                    .width(Length::Fill)
                    .align_y(Alignment::Center)
                    .height(Length::Fixed(16.0)),
            )
            .on_enter(Message::SpaceHint(true))
            .on_exit(Message::SpaceHint(false))
            .on_press(Message::AddSpace)
            .interaction(mouse::Interaction::Pointer);
            let list: Element<'a, Message> =
                crate::probe::probe(col.width(Length::Fill), self.tag_area.clone()).into();
            // The drop indicator floats over the rows at its gliding y, so
            // nothing shifts while it travels between gaps.
            let list: Element<'a, Message> = if drag.is_some_and(|d| d.target.is_some())
                && self.tag_line.shown()
            {
                let line = widget::container(retro::slot_line(p))
                    .padding(cosmic::iced::Padding {
                        top: self.tag_line.pos.max(0.0),
                        ..Default::default()
                    })
                    .width(Length::Fill);
                cosmic::iced::widget::stack([list, line.into()])
                    .width(Length::Fill)
                    .into()
            } else {
                list
            };
            widget::column::with_capacity(2)
                .push(self.hover_scroll(
                    ScrollArea::Tags,
                    widget::scrollable(
                        widget::container(list)
                            .padding([0, 10, 0, 0])
                            .width(Length::Fill),
                    )
                    .height(Length::Fill),
                ))
                .push(strip)
                .height(Length::Fill)
                .into()
        };
        retro::pane(p, self.title_font(), fl!("frame-tags"), None, body, p.bg)
    }

    /// Wrap a scrollable so its bar appears on hover and hides otherwise.
    fn hover_scroll<'a>(
        &self,
        area: ScrollArea,
        s: cosmic::iced::widget::Scrollable<'a, Message, cosmic::Theme, cosmic::Renderer>,
    ) -> Element<'a, Message> {
        let on = self.scroll_hover == Some(area);
        let w = if on { 8.0 } else { 0.0 };
        widget::mouse_area(
            s.scrollbar_width(w)
                .scroller_width(w)
                .scrollbar_padding(if on { 2.0 } else { 0.0 }),
        )
        .on_enter(Message::ScrollHover(area, true))
        .on_exit(Message::ScrollHover(area, false))
        .into()
    }

    fn notes_frame<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let total = match &self.view {
            View::All => self.view_counts[0],
            View::Untagged => self.view_counts[1],
            View::Trash => self.view_counts[2],
            View::Tag(_) if self.query.is_empty() => self.notes.len(),
            View::Tag(t) => self
                .store
                .as_ref()
                .and_then(|s| s.list(&View::Tag(t.clone())).ok())
                .map_or(self.notes.len(), |v| v.len()),
        };
        let badge = if self.query.is_empty() {
            fl!("notes-count", count = total)
        } else {
            fl!("notes-of", shown = self.notes.len(), total = total)
        };

        let search = widget::text_input(fl!("search-placeholder"), &self.query)
            .id(self.search_id.clone())
            .font(self.list_font())
            .size(f32::from(self.list_size))
            .padding([5, 8])
            .leading_icon(
                widget::container(retro::accent(p, "/"))
                    .padding([0, 0, 0, 8])
                    .into(),
            )
            .style(retro::search_class(p))
            .on_input(Message::Search);

        let body: Element<'_, Message> = if self.notes.is_empty() {
            let text = if self.query.is_empty() {
                fl!("no-notes-here")
            } else {
                fl!("no-results", query = self.query.as_str())
            };
            widget::container(retro::dim(p, text))
                .padding(12)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into()
        } else {
            let mut col = widget::column::with_capacity(self.notes.len()).spacing(4);
            for note in &self.notes {
                let selected = self.current.as_ref().is_some_and(|c| c.id == note.id);
                col = col.push(
                    widget::button::custom(note_row(
                        p,
                        note,
                        selected,
                        self.list_size,
                        self.list_font(),
                    ))
                    .padding([7, 8])
                    .width(Length::Fill)
                    .class(retro::row_class(p, selected))
                    .on_press(Message::Select(note.id.clone())),
                );
            }
            self.hover_scroll(
                ScrollArea::Notes,
                widget::scrollable(col.padding([0, 12, 0, 0]))
                    .id(self.notes_scroll_id.clone())
                    .height(Length::Fill),
            )
        };

        let content = widget::column::with_capacity(2)
            .push(search)
            .push(body)
            .spacing(8)
            .width(Length::Fill)
            .height(Length::Fill);
        retro::pane(
            p,
            self.title_font(),
            fl!("frame-notes"),
            Some(badge),
            content,
            p.panel,
        )
    }

    fn editor_frame<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let Some(note) = &self.current else {
            let hint = widget::container(retro::dim(p, fl!("no-note-selected")))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center);
            let content = widget::column::with_capacity(2)
                .push(hint)
                .push(self.dock(p))
                .spacing(8)
                .width(Length::Fill)
                .height(Length::Fill);
            return retro::pane(
                p,
                self.title_font(),
                fl!("app-title").to_lowercase(),
                None,
                content,
                p.panel,
            );
        };
        // Quiet by default: a tick. Click it for the last save time; a failed
        // save replaces it with a warning until the next one succeeds.
        let last_saved = format_time(note.modified);
        let badge: Element<'a, Message> = if self.drop_hover {
            retro::dim(p, fl!("badge-drop")).into()
        } else if note.trashed {
            retro::dim(p, fl!("badge-in-trash")).into()
        } else if self.save_error.is_some() {
            widget::tooltip(
                widget::button::custom(
                    retro::accent(p, fl!("badge-not-saved", time = last_saved))
                        .size(12)
                        .wrapping(cosmic::iced::widget::text::Wrapping::None),
                )
                .padding([0, 4])
                .class(retro::row_class(p, false))
                .on_press(Message::ToggleSavedInfo),
                retro::dim(p, self.save_error.clone().unwrap_or_default()),
                widget::tooltip::Position::Bottom,
            )
            .into()
        } else {
            let label = if self.saved_info_until.is_some() {
                fl!("badge-saved", time = last_saved)
            } else {
                "✓".to_owned()
            };
            widget::button::custom(
                retro::dim(p, label)
                    .size(12)
                    .wrapping(cosmic::iced::widget::text::Wrapping::None),
            )
            .padding([0, 4])
            .class(retro::row_class(p, false))
            .on_press(Message::ToggleSavedInfo)
            .into()
        };

        let text_area: Element<'a, Message> = self.blocks_view(p, note.trashed);
        let mut column = widget::column::with_capacity(3)
            .push(text_area)
            .spacing(8)
            .width(Length::Fill)
            .height(Length::Fill);
        if note.trashed {
            column =
                column.push(widget::container(retro::dim(p, fl!("trash-hint"))).padding([6, 10]));
        }
        column = column.push(self.dock(p));

        let framed = retro::pane_el(
            p,
            self.title_font(),
            String::new(),
            Some(badge),
            column,
            p.panel,
        );
        widget::dnd_destination::dnd_destination_for_data::<UriList, Message>(
            framed,
            |data, _action| Message::Dropped(data),
        )
        .on_enter(|_, _, _| Message::DragEnter)
        .on_motion(|x, y| Message::DragMotion(x as f32, y as f32))
        .on_leave(|| Message::DragLeave)
        // The drag also reaches the editors as pointer motion, translated by
        // the scroll container on the way: that is the position `drop_line_at`
        // reads, since the raw offer coordinates know nothing about scrolling.
        .forward_drag_as_cursor(true)
        .into()
    }

    fn backlinks_frame<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let mut row = widget::row::with_capacity(self.backlinks.len() + 1)
            .spacing(6)
            .align_y(Alignment::Center)
            .push(retro::dim(p, "←").class(cosmic::theme::Text::Color(p.mute)));
        for link in &self.backlinks {
            row = row.push(
                widget::button::custom(retro::accent2(p, link.title.clone()))
                    .padding([2, 6])
                    .class(retro::row_class(p, false))
                    .on_press(Message::Select(link.id.clone())),
            );
        }
        retro::pane(
            p,
            self.title_font(),
            fl!("linked-from").to_lowercase(),
            None,
            self.hover_scroll(
                ScrollArea::Backlinks,
                widget::scrollable(row).direction(
                    cosmic::iced::widget::scrollable::Direction::Horizontal(
                        cosmic::iced::widget::scrollable::Scrollbar::default(),
                    ),
                ),
            ),
            p.panel,
        )
    }

    /// Apply a text size to a pane and remember it in the config.
    fn set_pane_size(&mut self, pane: Pane, next: u16) {
        let saved = match (pane, &self.config_handler) {
            (Pane::Editor, Some(h)) => {
                self.font_size = next;
                self.config.set_editor_font_size(h, next).map(|_| ())
            }
            (Pane::Sidebar, Some(h)) => {
                self.sidebar_size = next;
                self.config.set_sidebar_font_size(h, next).map(|_| ())
            }
            (Pane::List, Some(h)) => {
                self.list_size = next;
                self.config.set_list_font_size(h, next).map(|_| ())
            }
            (Pane::Editor, None) => {
                self.font_size = next;
                Ok(())
            }
            (Pane::Sidebar, None) => {
                self.sidebar_size = next;
                Ok(())
            }
            (Pane::List, None) => {
                self.list_size = next;
                Ok(())
            }
        };
        if let Err(why) = saved {
            tracing::warn!(%why, ?pane, "saving text size");
        }
    }

    /// The dock: format actions for the open note, `+`, and the theme
    /// picker, as a centred pill at the foot of the editor. The `+` section
    /// opens as a second pill underneath so the main row never overflows.
    fn dock<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let editable = self.current.as_ref().is_some_and(|n| !n.trashed);
        let divider = || {
            widget::container(retro::dim(p, "│").class(cosmic::theme::Text::Color(p.mute)))
                .padding([0, 1])
        };
        let ds = self.dock_size;
        let icon = |i: DockIcon, colour: cosmic::iced::Color| {
            widget::svg(i.handle(self.icon_set, colour))
                .width(ds.icon())
                .height(ds.icon())
        };
        // A wrapping row, so the bigger sizes fold onto extra lines instead
        // of running off the pane.
        let mut items: Vec<Element<'a, Message>> = Vec::with_capacity(16);

        for format in Format::ALL {
            let button = widget::button::custom(icon(format.icon(), p.fg))
                .padding(ds.pad())
                .class(retro::row_class(p, false))
                .on_press_maybe(editable.then_some(Message::Format(format)));
            items.push(
                widget::tooltip(
                    button,
                    retro::dim(
                        p,
                        self.with_shortcut(format.label(), MenuAction::Format(format)),
                    ),
                    widget::tooltip::Position::Top,
                )
                .into(),
            );
        }

        items.push(divider().into());
        items.push(
            widget::tooltip(
                widget::button::custom(icon(DockIcon::Plus, p.accent))
                    .padding(ds.pad())
                    .class(retro::row_class(p, self.dock_open))
                    .on_press(Message::ToggleDock),
                retro::dim(p, self.with_shortcut(fl!("dock-plus"), MenuAction::NewNote)),
                widget::tooltip::Position::Top,
            )
            .into(),
        );

        items.push(
            widget::tooltip(
                widget::button::custom(icon(DockIcon::Image, p.accent))
                    .padding(ds.pad())
                    .class(retro::row_class(p, false))
                    .on_press_maybe(editable.then_some(Message::PickImage)),
                retro::dim(
                    p,
                    self.with_shortcut(fl!("dock-image"), MenuAction::AddImage),
                ),
                widget::tooltip::Position::Top,
            )
            .into(),
        );

        let row = widget::flex_row(items)
            .spacing(1)
            .align_items(Alignment::Center);

        let pill = |content: Element<'a, Message>| {
            widget::container(
                widget::container(content)
                    .padding(ds.pill())
                    .class(retro::dock_class(p)),
            )
            .width(Length::Fill)
            .align_x(Alignment::Center)
        };

        let mut dock = widget::column::with_capacity(2)
            .push(pill(row.into()))
            .spacing(6)
            .width(Length::Fill);

        if self.dock_open {
            let create = widget::row::with_capacity(3)
                .push(
                    widget::button::custom(retro::text(p, fl!("dock-new-note")))
                        .padding([3, 8])
                        .class(retro::row_class(p, false))
                        .on_press(Message::NewNote),
                )
                .push(
                    widget::text_input(fl!("folder-name-placeholder"), &self.new_folder)
                        .id(self.folder_id.clone())
                        .font(retro::mono())
                        .size(13)
                        .padding([3, 8])
                        .width(Length::Fixed(170.0))
                        .leading_icon(
                            widget::container(retro::accent2(p, "#"))
                                .padding([0, 0, 0, 8])
                                .into(),
                        )
                        .style(retro::search_class(p))
                        .on_input(Message::NewFolderName)
                        .on_submit(|_| Message::CreateFolder),
                )
                .push(
                    widget::button::custom(retro::text(p, fl!("dock-new-folder")))
                        .padding([3, 8])
                        .class(retro::row_class(p, false))
                        .on_press_maybe(
                            (!self.new_folder.trim().is_empty()).then_some(Message::CreateFolder),
                        ),
                )
                .spacing(4)
                .align_y(Alignment::Center);
            dock = dock.push(pill(create.into()));
        }

        dock.into()
    }

    /// The theme picker shown in the context drawer.
    /// The Appearance drawer: Colour, Font and Size, each foldable.
    fn theme_picker(&self) -> Element<'_, Message> {
        let system = self.core.system_theme();
        let label_color: cosmic::iced::Color = system.cosmic().background(false).on.into();
        let header = |label: String, section: Section| {
            let open = self.appearance_open[section as usize];
            widget::button::custom(
                widget::row::with_capacity(2)
                    .push(widget::text(if open { "▾" } else { "▸" }).size(16))
                    .push(widget::text::heading(label))
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .padding([6, 4])
            .width(Length::Fill)
            .class(cosmic::theme::Button::Text)
            .on_press(Message::ToggleSection(section))
        };
        let mut col = widget::column::with_capacity(40)
            .spacing(6)
            .width(Length::Fill);

        // Colour.
        col = col.push(header(fl!("section-colour"), Section::Colour));
        if self.appearance_open[Section::Colour as usize] {
            col = col.push(widget::text::caption(fl!("theme-picker-hint")));
            let swatch = |theme: retro::Theme| {
                retro::swatch(
                    theme,
                    &theme.palette(system),
                    !self.buffet_on && self.theme == theme,
                    label_color,
                    Message::SetTheme(theme),
                )
            };
            for theme in self.themes().into_iter().filter(|t| !t.is_minimal()) {
                col = col.push(swatch(theme));
            }
            col = col.push(
                widget::container(widget::text::heading(fl!("theme-minimal-header")))
                    .padding([10, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("theme-minimal-hint")));
            for theme in retro::Theme::ALL.into_iter().filter(|t| t.is_minimal()) {
                col = col.push(swatch(theme));
            }
        }

        // Colour buffet: mix any highlight with any dark plate, live.
        col = col.push(
            widget::container(header(fl!("section-buffet"), Section::Buffet)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Buffet as usize] {
            col = col.push(widget::text::caption(fl!("buffet-hint")));
            col = col.push(
                widget::container(widget::text(fl!("buffet-highlight"))).padding([6, 0, 2, 0]),
            );
            let mut chips: Vec<Element<'_, Message>> = Vec::new();
            for theme in retro::Theme::ALL.into_iter().filter(|t| t.is_minimal()) {
                if let Some(colour) = theme.minimal_accent() {
                    chips.push(retro::chip(
                        colour,
                        self.buffet_highlight == theme,
                        label_color,
                        Message::SetBuffetHighlight(theme),
                    ));
                }
            }
            col = col.push(widget::flex_row(chips).spacing(6));

            // Dark mode: a dark plate, light ink comes with it.
            col = col.push(
                widget::container(widget::text::heading(fl!("buffet-dark-mode")))
                    .padding([10, 0, 2, 0]),
            );
            let dark_active = !self.buffet_light_mode;
            let mut plates: Vec<Element<'_, Message>> = Vec::new();
            for dark in retro::Dark::ALL {
                plates.push(retro::chip(
                    dark.color(),
                    dark_active && self.buffet_dark == dark,
                    label_color,
                    Message::SetBuffetDark(dark),
                ));
            }
            col = col.push(widget::flex_row(plates).spacing(6));

            // Light mode: a paper plate and a writing ink.
            col = col.push(
                widget::container(widget::text::heading(fl!("buffet-light-mode")))
                    .padding([10, 0, 2, 0]),
            );
            col =
                col.push(widget::container(widget::text(fl!("buffet-paper"))).padding([2, 0, 2, 0]));
            let mut papers: Vec<Element<'_, Message>> = Vec::new();
            for light in retro::Light::ALL {
                papers.push(retro::chip(
                    light.color(),
                    self.buffet_light_mode && self.buffet_light == light,
                    label_color,
                    Message::SetBuffetLight(light),
                ));
            }
            col = col.push(widget::flex_row(papers).spacing(6));
            col =
                col.push(widget::container(widget::text(fl!("buffet-ink"))).padding([6, 0, 2, 0]));
            let mut inks: Vec<Element<'_, Message>> = Vec::new();
            for ink in retro::Ink::ALL {
                inks.push(retro::chip(
                    ink.color(),
                    self.buffet_light_mode && self.buffet_ink == ink,
                    label_color,
                    Message::SetBuffetInk(ink),
                ));
            }
            col = col.push(widget::flex_row(inks).spacing(6));

            let status = if self.buffet_on {
                if self.buffet_light_mode {
                    fl!(
                        "buffet-serving-light",
                        hi = self.buffet_highlight.label(),
                        paper = self.buffet_light.label(),
                        ink = self.buffet_ink.label()
                    )
                } else {
                    fl!(
                        "buffet-serving-dark",
                        hi = self.buffet_highlight.label(),
                        plate = self.buffet_dark.label()
                    )
                }
            } else {
                fl!("buffet-off")
            };
            col = col.push(widget::container(widget::text::caption(status)).padding([6, 0, 0, 0]));
        }

        // Tables.
        col = col.push(
            widget::container(header(fl!("section-tables"), Section::Tables)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Tables as usize] {
            col = col.push(
                widget::container(
                    widget::toggler(!self.config.table_toolbar_off)
                        .label(fl!("table-toolbar-toggle"))
                        .on_toggle(Message::SetTableToolbar),
                )
                .padding([4, 0]),
            );
            col = col.push(widget::text::caption(fl!("table-toolbar-hint")));
        }

        // Font.
        col = col.push(
            widget::container(header(fl!("section-font"), Section::Font)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Font as usize] {
            let font_row = |font: retro::EditorFont| {
                let selected = font
                    == match self.font_target {
                        Pane::Editor => self.editor_font,
                        Pane::Sidebar => self.ui_font,
                        Pane::List => self.list_font,
                    };
                let sample = widget::column::with_capacity(2)
                    .push(widget::text(font.label()).font(font.font()).size(17))
                    .push(widget::text::caption(font.blurb()))
                    .spacing(2);
                widget::button::custom(widget::container(sample).width(Length::Fill))
                    .padding([6, 10])
                    .width(Length::Fill)
                    .class(if selected {
                        cosmic::theme::Button::Suggested
                    } else {
                        cosmic::theme::Button::Standard
                    })
                    .on_press(Message::SetFont(font))
            };
            // Which pane the font clicks below act on, with its size.
            col = col.push(widget::text::heading(fl!("font-target")));
            let mut targets = widget::row::with_capacity(3).spacing(6);
            for (pane, label) in [
                (Pane::Sidebar, fl!("target-tags")),
                (Pane::List, fl!("target-note")),
                (Pane::Editor, fl!("target-editor")),
            ] {
                targets = targets.push(
                    widget::button::custom(widget::text(label))
                        .padding([4, 12])
                        .class(if self.font_target == pane {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetFontTarget(pane)),
                );
            }
            col = col.push(targets);
            let (size, min, max) = match self.font_target {
                Pane::Editor => (self.font_size, retro::FONT_SIZE_MIN, retro::FONT_SIZE_MAX),
                Pane::Sidebar => (
                    self.sidebar_size,
                    retro::PANE_SIZE_MIN,
                    retro::PANE_SIZE_MAX,
                ),
                Pane::List => (self.list_size, retro::PANE_SIZE_MIN, retro::PANE_SIZE_MAX),
            };
            let tstep = |glyph: &'static str, delta: i16, enabled: bool| {
                widget::button::custom(widget::text(glyph).size(16))
                    .padding([0, 10])
                    .class(cosmic::theme::Button::Standard)
                    .on_press_maybe(
                        enabled.then_some(Message::SizeStep(self.font_target, delta)),
                    )
            };
            col = col.push(
                widget::row::with_capacity(4)
                    .push(
                        widget::text(fl!("font-size-label"))
                            .size(14)
                            .width(Length::Fixed(70.0)),
                    )
                    .push(tstep("−", -1, size > min))
                    .push(widget::text(format!("{size} px")).size(14))
                    .push(tstep("+", 1, size < max))
                    .spacing(8)
                    .align_y(Alignment::Center),
            );
            // The staples up top, the long tail behind a second heading,
            // and the pairings — the chef's selection — to finish.
            col = col.push(widget::text::heading(fl!("font-base")));
            for font in retro::BASE_FONTS {
                col = col.push(font_row(font));
            }
            col = col.push(
                widget::container(widget::text::heading(fl!("font-weight"))).padding([6, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("font-weight-hint")));
            let mut weights = widget::row::with_capacity(4).spacing(6);
            for w in [200u16, 300, 400, 500] {
                let face = cosmic::font::Font {
                    weight: match w {
                        200 => cosmic::iced::font::Weight::ExtraLight,
                        300 => cosmic::iced::font::Weight::Light,
                        500 => cosmic::iced::font::Weight::Medium,
                        _ => cosmic::iced::font::Weight::Normal,
                    },
                    ..self.editor_font.font()
                };
                weights = weights.push(
                    widget::button::custom(widget::text(w.to_string()).font(face).size(15))
                        .padding([4, 12])
                        .class(if self.editor_weight == w {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetWeight(w)),
                );
            }
            col = col.push(weights);
            col = col.push(
                widget::container(widget::text::heading(fl!("font-more"))).padding([8, 0, 0, 0]),
            );
            for font in retro::EditorFont::ALL {
                if retro::BASE_FONTS.contains(&font) {
                    continue;
                }
                col = col.push(font_row(font));
            }
            col = col.push(
                widget::container(widget::text::heading(fl!("font-chefs"))).padding([8, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("font-pairings-hint")));
            let current = retro::PAIRINGS.iter().find(|pr| {
                pr.title == self.title_font && pr.ui == self.ui_font && pr.body == self.editor_font
            });
            for pair in &retro::PAIRINGS {
                let selected = current == Some(pair);
                let title_font = if pair.title.has_bold() {
                    cosmic::font::Font {
                        weight: cosmic::iced::font::Weight::Bold,
                        ..pair.title.font()
                    }
                } else {
                    pair.title.font()
                };
                let sample = widget::column::with_capacity(4)
                    .push(widget::text(pair.name).font(title_font).size(20))
                    .push(
                        widget::text(fl!("pairing-sample-ui"))
                            .font(pair.ui.font())
                            .size(13),
                    )
                    .push(
                        widget::text(fl!("pairing-sample-body"))
                            .font(pair.body.font())
                            .size(15),
                    )
                    .push(widget::text::caption(pair.blurb))
                    .spacing(3);
                col = col.push(
                    widget::button::custom(widget::container(sample).width(Length::Fill))
                        .padding([8, 10])
                        .width(Length::Fill)
                        .class(if selected {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetPairing(pair.key.to_owned())),
                );
            }
            col = col.push(
                widget::button::custom(widget::text(fl!("font-restore")))
                    .padding([6, 12])
                    .class(cosmic::theme::Button::Standard)
                    .on_press(Message::RestoreFonts),
            );
        }

        // Size: one card per pane with a live sample, then the dock.
        col = col.push(
            widget::container(header(fl!("section-size"), Section::Size)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Size as usize] {
            let cards = [
                (
                    Pane::Sidebar,
                    fl!("size-sidebar"),
                    fl!("size-sample-sidebar"),
                    self.sidebar_size,
                    retro::mono(),
                    retro::PANE_SIZE_MIN,
                    retro::PANE_SIZE_MAX,
                ),
                (
                    Pane::List,
                    fl!("size-list"),
                    fl!("size-sample-list"),
                    self.list_size,
                    retro::mono(),
                    retro::PANE_SIZE_MIN,
                    retro::PANE_SIZE_MAX,
                ),
                (
                    Pane::Editor,
                    fl!("size-editor"),
                    fl!("size-sample-editor"),
                    self.font_size,
                    self.body_font(),
                    retro::FONT_SIZE_MIN,
                    retro::FONT_SIZE_MAX,
                ),
            ];
            for (pane, label, sample, size, font, min, max) in cards {
                let step = |glyph: &'static str, delta: i16, enabled: bool| {
                    widget::button::custom(widget::text(glyph).size(16))
                        .padding([0, 10])
                        .class(cosmic::theme::Button::Standard)
                        .on_press_maybe(enabled.then_some(Message::SizeStep(pane, delta)))
                };
                let controls = widget::row::with_capacity(4)
                    .push(widget::text::heading(label).width(Length::Fill))
                    .push(step("−", -1, size > min))
                    .push(widget::text(format!("{size} px")).size(14))
                    .push(step("+", 1, size < max))
                    .spacing(8)
                    .align_y(Alignment::Center);
                let card = widget::column::with_capacity(2)
                    .push(controls)
                    .push(widget::text(sample).font(font).size(f32::from(size)))
                    .spacing(6)
                    .width(Length::Fill);
                col = col.push(
                    widget::container(card)
                        .padding([8, 12])
                        .width(Length::Fill)
                        .class(cosmic::theme::Container::Card),
                );
            }

            col = col.push(
                widget::container(widget::text::heading(fl!("dock-size"))).padding([6, 0, 0, 0]),
            );
            let mut sizes = widget::row::with_capacity(4).spacing(6);
            for size in retro::DockSize::ALL {
                sizes = sizes.push(
                    widget::button::custom(widget::text(size.label()))
                        .padding([4, 10])
                        .class(if self.dock_size == size {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetDockSize(size)),
                );
            }
            col = col.push(sizes);

            col = col.push(
                widget::container(widget::text::heading(fl!("text-width"))).padding([6, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("text-width-hint")));
            let mut widths = widget::row::with_capacity(4).spacing(6);
            for m in retro::Measure::ALL {
                widths = widths.push(widget::tooltip(
                    widget::button::custom(widget::text(m.label()).size(15))
                        .padding([4, 10])
                        .class(if self.measure == m {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetMeasure(m)),
                    widget::text::caption(m.blurb()),
                    widget::tooltip::Position::Top,
                ));
            }
            col = col.push(widths);

            // Line weights: the --- divider, and the border around cards.
            let line_row = |current: retro::LineSize,
                            msg: fn(retro::LineSize) -> Message| {
                let mut row = widget::row::with_capacity(4).spacing(6);
                for s in retro::LineSize::ALL {
                    row = row.push(
                        widget::button::custom(widget::text(s.label()))
                            .padding([4, 10])
                            .class(if current == s {
                                cosmic::theme::Button::Suggested
                            } else {
                                cosmic::theme::Button::Standard
                            })
                            .on_press(msg(s)),
                    );
                }
                row
            };
            col = col.push(
                widget::container(widget::text::heading(fl!("size-rule"))).padding([6, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("size-rule-hint")));
            col = col.push(line_row(self.rule_size, Message::SetRuleSize));
            col = col.push(
                widget::container(widget::text::heading(fl!("size-tag-line")))
                    .padding([6, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("size-tag-line-hint")));
            col = col.push(line_row(self.tag_line_size, Message::SetTagLineSize));
            col = col.push(
                widget::container(widget::text::heading(fl!("size-card"))).padding([6, 0, 0, 0]),
            );
            col = col.push(
                widget::container(
                    widget::toggler(self.card_border)
                        .label(fl!("card-border-toggle"))
                        .on_toggle(Message::SetCardBorder),
                )
                .padding([4, 0]),
            );
            if self.card_border {
                col = col.push(line_row(self.card_line, Message::SetCardLine));
            }

            col = col.push(
                widget::container(widget::text::heading(fl!("animation"))).padding([6, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("animation-hint")));
            let anim_step = |glyph: &'static str, msg: Option<Message>| {
                widget::button::custom(widget::text(glyph).size(16))
                    .padding([0, 10])
                    .class(cosmic::theme::Button::Standard)
                    .on_press_maybe(msg)
            };
            let time_shown = if self.anim_ms == 0 {
                fl!("anim-off")
            } else {
                format!("{} ms", self.anim_ms)
            };
            col = col.push(
                widget::row::with_capacity(4)
                    .push(widget::text(fl!("anim-time")).size(14).width(Length::Fixed(70.0)))
                    .push(anim_step(
                        "−",
                        (self.anim_ms > 0).then_some(Message::AnimStep(-1)),
                    ))
                    .push(widget::text(time_shown).size(14))
                    .push(anim_step(
                        "+",
                        (self.anim_ms < crate::anim::MAX_MS).then_some(Message::AnimStep(1)),
                    ))
                    .spacing(8)
                    .align_y(Alignment::Center),
            );
            let ease = f32::from(self.anim_ease10) / 10.0;
            col = col.push(
                widget::row::with_capacity(4)
                    .push(
                        widget::text(fl!("anim-landing"))
                            .size(14)
                            .width(Length::Fixed(70.0)),
                    )
                    .push(anim_step(
                        "−",
                        (self.anim_ease10 > crate::anim::MIN_EASE10)
                            .then_some(Message::AnimEaseStep(-1)),
                    ))
                    .push(widget::text(format!("{ease:.1}")).size(14))
                    .push(anim_step(
                        "+",
                        (self.anim_ease10 < crate::anim::MAX_EASE10)
                            .then_some(Message::AnimEaseStep(1)),
                    ))
                    .spacing(8)
                    .align_y(Alignment::Center),
            );
        }

        // Tasks: what a finished one is marked with.
        col = col.push(
            widget::container(header(fl!("section-tasks"), Section::Tasks)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Tasks as usize] {
            col = col.push(widget::text::caption(fl!("task-marker-hint")));
            let mut marks: Vec<Element<'_, Message>> =
                Vec::with_capacity(retro::TASK_MARKERS.len());
            for (mark, blurb) in retro::TASK_MARKERS {
                let selected = self.task_marker == mark;
                marks.push(
                    widget::tooltip(
                        widget::button::custom(
                            widget::text(format!("[{mark}]"))
                                .font(retro::mono())
                                .size(16),
                        )
                        .padding([4, 8])
                        .class(if selected {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetTaskMarker(mark.to_owned())),
                        widget::text::caption(blurb),
                        widget::tooltip::Position::Top,
                    )
                    .into(),
                );
            }
            col = col.push(widget::flex_row(marks).spacing(6));
            let p = self.palette();
            let sample = widget::column::with_capacity(2)
                .push(
                    retro::text(&p, fl!("task-sample-open"))
                        .font(self.body_font())
                        .size(f32::from(self.font_size)),
                )
                .push(
                    retro::text(
                        &p,
                        fl!("task-sample-done", mark = self.task_marker.as_str()),
                    )
                    .font(self.body_font())
                    .size(f32::from(self.font_size))
                    .class(cosmic::theme::Text::Color(p.fg.scale_alpha(0.45))),
                )
                .spacing(4);
            col = col.push(
                widget::container(sample)
                    .padding([8, 12])
                    .width(Length::Fill)
                    .class(cosmic::theme::Container::Card),
            );
        }

        // Icon: the launcher tile in any theme's colours, or following the theme.
        col = col.push(
            widget::container(header(fl!("section-icon"), Section::Icon)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Icon as usize] {
            col = col.push(widget::text::caption(fl!("icon-hint")));
            col = col.push(
                widget::container(
                    widget::toggler(self.icon_theme.is_none())
                        .label(fl!("icon-follow"))
                        .on_toggle(|on| {
                            Message::SetIcon(if on {
                                None
                            } else {
                                Some(retro::Theme::Phosphor)
                            })
                        }),
                )
                .padding([4, 0, 8, 0]),
            );
            let mut tiles: Vec<Element<'_, Message>> = Vec::with_capacity(retro::Theme::ALL.len());
            // Following the buffet, no theme tile is "the" icon.
            let current = self
                .icon_theme
                .or_else(|| (!self.buffet_on).then_some(self.theme));
            for theme in self.themes() {
                let svg = crate::icon::svg(&theme.palette(system));
                let handle = widget::svg::Handle::from_memory(svg.into_bytes());
                let selected = current == Some(theme);
                tiles.push(
                    widget::tooltip(
                        widget::button::custom(widget::svg(handle).width(52).height(52))
                            .padding(2)
                            .class(if selected {
                                cosmic::theme::Button::Suggested
                            } else {
                                cosmic::theme::Button::Text
                            })
                            .on_press(Message::SetIcon(Some(theme))),
                        widget::text::caption(theme.label()),
                        widget::tooltip::Position::Top,
                    )
                    .into(),
                );
            }
            col = col.push(widget::flex_row(tiles).spacing(4));

            // Folder icon style: one card per set, previewed with a few glyphs.
            col = col.push(
                widget::container(widget::text::heading(fl!("icon-set"))).padding([10, 0, 0, 0]),
            );
            col = col.push(widget::text::caption(fl!("icon-set-hint")));
            let mut sets =
                widget::column::with_capacity(crate::glyph::IconSet::ALL.len()).spacing(6);
            let sample = [
                crate::glyph::Icon::Coffee,
                crate::glyph::Icon::Star,
                crate::glyph::Icon::Plane,
                crate::glyph::Icon::Idea,
                crate::glyph::Icon::Heart,
                crate::glyph::Icon::Book,
                crate::glyph::Icon::Camera,
                crate::glyph::Icon::Music,
            ];
            for set in crate::glyph::IconSet::ALL {
                let mut glyphs = widget::row::with_capacity(sample.len()).spacing(6);
                for icon in sample {
                    glyphs = glyphs.push(
                        widget::svg(icon.handle(set, label_color))
                            .width(20)
                            .height(20),
                    );
                }
                let header = widget::row::with_capacity(2)
                    .push(widget::text(set.label()).width(Length::Fill))
                    .push(widget::text::caption(set.licence()))
                    .align_y(Alignment::Center);
                let card = widget::column::with_capacity(3)
                    .push(header)
                    .push(glyphs)
                    .push(widget::text::caption(set.blurb()))
                    .spacing(4);
                sets = sets.push(
                    widget::button::custom(widget::container(card).width(Length::Fill))
                        .padding([8, 12])
                        .width(Length::Fill)
                        .class(if self.icon_set == set {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        })
                        .on_press(Message::SetIconSet(set)),
                );
            }
            col = col.push(sets);
        }

        // Links: whether web addresses fetch a preview.
        col = col.push(
            widget::container(header(fl!("section-links"), Section::Links)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Links as usize] {
            col = col.push(
                widget::toggler(self.link_previews)
                    .label(fl!("link-previews"))
                    .on_toggle(Message::SetLinkPreviews),
            );
            col = col.push(widget::text::caption(fl!("link-previews-hint")));
            col = col.push(widget::text::caption(fl!("attach-hint")));
        }

        // Sync: the account on a self-hosted server.
        col = col.push(
            widget::container(header(fl!("section-sync"), Section::Sync)).padding([8, 0, 0, 0]),
        );
        if self.appearance_open[Section::Sync as usize] {
            col = col.push(self.sync_section());
        }
        self.hover_scroll(ScrollArea::Options, widget::scrollable(col))
    }

    /// Options → Sync: the sign-in form, or the account and its status.
    fn sync_section(&self) -> Element<'_, Message> {
        let mut col = widget::column::with_capacity(10).spacing(8);
        let status = |text: String| widget::text::caption(text);
        match &self.sync.session {
            Some(session) => {
                col = col.push(widget::text::body(fl!(
                    "sync-signed-in",
                    email = session.email.clone(),
                    url = session.url.clone()
                )));
                let line = if self.sync.busy {
                    fl!("sync-syncing")
                } else if let Some(err) = &self.sync.error {
                    fl!("sync-error", error = err.clone())
                } else if let Some(at) = self.sync.last_ok {
                    fl!("sync-last", ago = ago(at.elapsed()))
                } else {
                    fl!("sync-waiting")
                };
                col = col.push(status(line));
                if let Some(summary) = &self.sync.summary {
                    col = col.push(status(summary.clone()));
                }
                if let Some(summary) = &self.sync.files_summary {
                    col = col.push(status(summary.clone()));
                }
                let pending = self.store.as_ref().map_or(0, Store::sync_pending_count);
                if pending > 0 && !self.sync.busy {
                    col = col.push(status(fl!("sync-pending", n = pending)));
                }
                let files_pending = self.store.as_ref().map_or(0, Store::sync_files_pending_count);
                if files_pending > 0 && !self.sync.busy {
                    col = col.push(status(fl!("sync-files-pending", n = files_pending)));
                }
                col = col.push(
                    widget::row::with_capacity(2)
                        .spacing(8)
                        .push(
                            widget::button::suggested(fl!("sync-now"))
                                .on_press_maybe((!self.sync.busy).then_some(Message::SyncNow)),
                        )
                        .push(widget::button::standard(fl!("sync-sign-out")).on_press(Message::SyncSignOut)),
                );
            }
            None => {
                col = col.push(widget::text::caption(fl!("sync-hint")));
                col = col.push(
                    widget::text_input(fl!("sync-url-placeholder"), &self.sync.url)
                        .label(fl!("sync-url"))
                        .on_input(Message::SyncUrl),
                );
                col = col.push(
                    widget::text_input(fl!("sync-email-placeholder"), &self.sync.email)
                        .label(fl!("sync-email"))
                        .on_input(Message::SyncEmail),
                );
                col = col.push(
                    widget::secure_input(
                        fl!("sync-password-placeholder"),
                        &self.sync.password,
                        Some(Message::SyncShowPassword),
                        !self.sync.show_password,
                    )
                    .label(fl!("sync-password"))
                    .on_input(Message::SyncPassword)
                    .on_submit(|_| Message::SyncSignIn),
                );
                let ready = !self.sync.busy
                    && !self.sync.restoring
                    && !self.sync.url.trim().is_empty()
                    && !self.sync.email.trim().is_empty()
                    && !self.sync.password.is_empty();
                col = col.push(
                    widget::row::with_capacity(2)
                        .spacing(8)
                        .push(
                            widget::button::suggested(fl!("sync-sign-in"))
                                .on_press_maybe(ready.then_some(Message::SyncSignIn)),
                        )
                        .push(
                            widget::button::standard(fl!("sync-create-account"))
                                .on_press_maybe(ready.then_some(Message::SyncSignUp)),
                        ),
                );
                if self.sync.busy || self.sync.restoring {
                    col = col.push(status(fl!("sync-signing-in")));
                } else if let Some(err) = &self.sync.error {
                    col = col.push(status(fl!("sync-error", error = err.clone())));
                }
                col = col.push(widget::text::caption(fl!("sync-server-hint")));
            }
        }
        col.into()
    }

    /// Apply a dock format action to the editor buffer.
    /// `sweep` is the highlighter pen (a middle-button drag): the swept
    /// range snaps to whole words and leaves no selection behind.
    fn apply_format(&mut self, format: Format, sweep: bool) {
        use text_editor::{Action, Edit, Motion};
        tracing::debug!(
            ?format,
            has_text = self.blocks.focused_text().is_some(),
            "applying format"
        );
        if self.blocks.focused_text().is_none() {
            return;
        }
        self.record(EditKind::Other);
        let Some(editor) = self.blocks.focused_text() else {
            return;
        };
        let perform = |editor: &mut crate::editor::Content, action: Action| editor.perform(action);
        let insert_str = |editor: &mut crate::editor::Content, s: &str| {
            for c in s.chars() {
                editor.perform(Action::Edit(if c == '\n' {
                    Edit::Enter
                } else {
                    Edit::Insert(c)
                }));
            }
        };

        match format {
            // The highlighter has its own rules: marks merge, never nest.
            Format::Mark if mark_selection(editor, sweep) => {}
            Format::Bold | Format::Italic | Format::Mark | Format::Code | Format::Link => {
                let (before, after) = match format {
                    Format::Bold => ("**", "**"),
                    Format::Italic => ("*", "*"),
                    Format::Mark => ("==", "=="),
                    Format::Code => ("`", "`"),
                    _ => ("[[", "]]"),
                };
                // A proper toggle, like every editor: wrap what's chosen,
                // unwrap it when it (or its surroundings) already carry the
                // markers, and step over a closing marker so Ctrl+B twice
                // means "start bold … stop bold".
                let cursor = editor.cursor();
                let line_no = cursor.position.line;
                let text = editor
                    .line(line_no)
                    .map(|l| l.text.into_owned())
                    .unwrap_or_default();
                let select =
                    |editor: &mut crate::editor::Content, a: usize, b: usize| {
                        editor.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: line_no,
                                column: b,
                            },
                            selection: Some(text_editor::Position {
                                line: line_no,
                                column: a,
                            }),
                        })
                    };
                let caret_to = |editor: &mut crate::editor::Content, col: usize| {
                    editor.move_to(text_editor::Cursor {
                        position: text_editor::Position {
                            line: line_no,
                            column: col,
                        },
                        selection: None,
                    })
                };
                // Replace `a..b` with `with`, then select `keep` inside the
                // result so the next press toggles straight back.
                let replace = |editor: &mut crate::editor::Content,
                               a: usize,
                               b: usize,
                               with: &str,
                               keep: (usize, usize)| {
                    select(editor, a, b);
                    perform(editor, Action::Edit(Edit::Paste(Arc::new(with.to_owned()))));
                    select(editor, keep.0, keep.1);
                };
                let one_line = cursor.selection.filter(|s| s.line == line_no).map(|s| {
                    (
                        s.column.min(cursor.position.column),
                        s.column.max(cursor.position.column),
                    )
                });
                match one_line {
                    Some((start, end))
                        if markdown::trim_range(&text, start, end).is_some() =>
                    {
                        // Markers only render hugging their text: a space
                        // swept up at either end stays outside them.
                        let std::ops::Range { start, end } =
                            markdown::trim_range(&text, start, end).unwrap_or(start..end);
                        let selected = text[start..end].to_owned();
                        let stripped = selected
                            .strip_prefix(before)
                            .and_then(|s| s.strip_suffix(after))
                            .filter(|s| !s.is_empty());
                        if let Some(inner) = stripped {
                            // The markers came along in the selection.
                            let inner = inner.to_owned();
                            replace(editor, start, end, &inner, (start, start + inner.len()));
                        } else if wrapped_in(&text, start, end, format, before, after) {
                            // Only the words were selected (the markers hide
                            // when rendered): eat the markers around them.
                            let a = start - before.len();
                            replace(editor, a, end + after.len(), &selected, (a, a + selected.len()));
                        } else {
                            let wrapped = format!("{before}{selected}{after}");
                            replace(
                                editor,
                                start,
                                end,
                                &wrapped,
                                (start + before.len(), end + before.len()),
                            );
                        }
                    }
                    _ if cursor.selection.is_some() => {
                        // A multi-line selection: wrap or strip it whole.
                        if let Some(selection) = editor.selection() {
                            let inner = selection
                                .strip_prefix(before)
                                .and_then(|s| s.strip_suffix(after))
                                .filter(|s| !s.is_empty());
                            let text = match inner {
                                Some(inner) => inner.to_owned(),
                                None => format!("{before}{selection}{after}"),
                            };
                            perform(editor, Action::Edit(Edit::Paste(Arc::new(text))));
                        }
                    }
                    _ => {
                        let col = cursor.position.column;
                        if text[col..].starts_with(after) && text[..col].contains(before) {
                            // On the closing marker: step out instead of
                            // planting a fresh pair.
                            for _ in 0..after.chars().count() {
                                perform(editor, Action::Move(Motion::Right));
                            }
                        } else if let Some((ws, we)) = word_bounds(&text, col) {
                            if wrapped_in(&text, ws, we, format, before, after) {
                                // Caret in a wrapped word: unwrap it.
                                let word = text[ws..we].to_owned();
                                let a = ws - before.len();
                                replace(editor, a, we + after.len(), &word, (0, 0));
                                caret_to(editor, col - before.len());
                            } else {
                                // Wrap the word under the caret; the caret
                                // stays on its character.
                                let wrapped = format!("{before}{}{after}", &text[ws..we]);
                                replace(editor, ws, we, &wrapped, (0, 0));
                                caret_to(editor, col + before.len());
                            }
                        } else {
                            // Open air: plant the pair, caret in the middle.
                            insert_str(editor, before);
                            insert_str(editor, after);
                            for _ in 0..after.chars().count() {
                                perform(editor, Action::Move(Motion::Left));
                            }
                        }
                    }
                }
            }
            Format::H1 | Format::H2 | Format::Bullet | Format::Todo | Format::Quote => {
                let prefix = match format {
                    Format::H1 => "# ",
                    Format::H2 => "## ",
                    Format::Bullet => "- ",
                    Format::Quote => "> ",
                    _ => "- [ ] ",
                };
                // Every line the selection touches (or just the cursor's).
                let cursor = editor.cursor();
                let (first, last) = match cursor.selection {
                    Some(sel) => (
                        sel.line.min(cursor.position.line),
                        sel.line.max(cursor.position.line),
                    ),
                    None => (cursor.position.line, cursor.position.line),
                };
                // Toggle: strip the same prefix if the line already carries it,
                // otherwise replace a different line prefix and add ours. With
                // several lines, the first line decides for all of them.
                let first_text = editor
                    .line(first)
                    .map(|l| l.text.into_owned())
                    .unwrap_or_default();
                let remove_only = line_prefix(&first_text).map(|(_, f)| f) == Some(format);
                for l in first..=last {
                    let line = editor
                        .line(l)
                        .map(|t| t.text.into_owned())
                        .unwrap_or_default();
                    editor.move_to(text_editor::Cursor {
                        position: text_editor::Position { line: l, column: 0 },
                        selection: None,
                    });
                    if let Some((len, _)) = line_prefix(&line) {
                        for _ in 0..line[..len].chars().count() {
                            perform(editor, Action::Edit(Edit::Delete));
                        }
                    }
                    if !remove_only {
                        insert_str(editor, prefix);
                    }
                }
                perform(editor, Action::Move(Motion::End));
            }
            Format::Tag => insert_str(editor, "#"),
            Format::Table => {
                let md = crate::table::Table::starter(3, 2).to_markdown();
                let cursor = editor.cursor();
                let line_empty = editor
                    .line(cursor.position.line)
                    .is_none_or(|l| l.text.trim().is_empty());
                perform(editor, Action::Move(Motion::End));
                if !line_empty {
                    insert_str(editor, "\n");
                }
                insert_str(editor, &md);
                insert_str(editor, "\n");
            }
            Format::Rule => {
                // A rule wants a line of its own; it is drawn as a full-width line.
                let cursor = editor.cursor();
                let line_empty = editor
                    .line(cursor.position.line)
                    .is_none_or(|l| l.text.trim().is_empty());
                if line_empty {
                    perform(editor, Action::Move(Motion::End));
                    insert_str(editor, "---\n");
                } else {
                    perform(editor, Action::Move(Motion::End));
                    insert_str(editor, "\n---\n");
                }
            }
        }
        self.dirty = true;
        self.last_edit = Instant::now();
        if self.blocks.needs_resplit() {
            self.blocks.resplit();
        }
    }

    /// In-app image picker: folders first, then image files.
    fn file_picker(&self) -> Element<'_, Message> {
        let mut col = widget::column::with_capacity(4)
            .spacing(8)
            .width(Length::Fill);

        let mut places = widget::row::with_capacity(3).spacing(6);
        for (icon_name, label, dir) in [
            ("user-home-symbolic", fl!("picker-home"), dirs::home_dir()),
            (
                "folder-pictures-symbolic",
                fl!("picker-pictures"),
                dirs::picture_dir(),
            ),
            (
                "folder-download-symbolic",
                fl!("picker-downloads"),
                dirs::download_dir(),
            ),
        ] {
            if let Some(dir) = dir {
                places = places.push(
                    widget::button::text(label)
                        .leading_icon(icon::from_name(icon_name))
                        .on_press(Message::PickerNavigate(dir)),
                );
            }
        }
        col = col.push(places);
        // Grid or list toggle beside the path.
        col = col.push(
            widget::row::with_capacity(2)
                .push(
                    widget::text::caption(self.picker_dir.display().to_string())
                        .width(Length::Fill),
                )
                .push(
                    widget::button::text(if self.picker_grid {
                        fl!("picker-list")
                    } else {
                        fl!("picker-grid")
                    })
                    .on_press(Message::PickerToggleView),
                )
                .align_y(Alignment::Center)
                .spacing(8),
        );
        if self.picker_entries.is_empty() {
            col = col.push(widget::text::caption(fl!("picker-empty")));
        }

        if self.picker_grid {
            // Folders as chips, then a grid of thumbnails.
            let mut folders: Vec<Element<'_, Message>> = Vec::new();
            if let Some(parent) = self.picker_dir.parent() {
                folders.push(
                    widget::button::text("..")
                        .leading_icon(icon::from_name("go-up-symbolic"))
                        .on_press(Message::PickerNavigate(parent.to_owned()))
                        .into(),
                );
            }
            for entry in self.picker_entries.iter().filter(|e| e.is_dir) {
                folders.push(
                    widget::button::text(entry.name.clone())
                        .leading_icon(icon::from_name("folder-symbolic"))
                        .on_press(Message::PickerNavigate(entry.path.clone()))
                        .into(),
                );
            }
            if !folders.is_empty() {
                col = col.push(widget::flex_row(folders).spacing(6));
            }
            let mut tiles: Vec<Element<'_, Message>> = Vec::new();
            for entry in self.picker_entries.iter().filter(|e| !e.is_dir) {
                let kind = crate::links::file_info(&entry.path).kind;
                let picture: Element<'_, Message> = match self.thumbs.get(&entry.path) {
                    None if !images::is_image_file(&entry.path) => {
                        widget::container(widget::text::heading(kind))
                            .width(Length::Fixed(104.0))
                            .height(Length::Fixed(78.0))
                            .align_x(Alignment::Center)
                            .align_y(Alignment::Center)
                            .into()
                    }
                    Some(ImageState::Ready(handle, _, _)) => widget::image(handle.clone())
                        .width(Length::Fixed(104.0))
                        .height(Length::Fixed(78.0))
                        .content_fit(cosmic::iced::ContentFit::Cover)
                        .into(),
                    Some(ImageState::Failed(_)) => {
                        widget::container(icon::from_name("image-missing-symbolic").size(28))
                            .width(Length::Fixed(104.0))
                            .height(Length::Fixed(78.0))
                            .align_x(Alignment::Center)
                            .align_y(Alignment::Center)
                            .into()
                    }
                    _ => widget::container(widget::text::caption("…"))
                        .width(Length::Fixed(104.0))
                        .height(Length::Fixed(78.0))
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center)
                        .into(),
                };
                let mut name = entry.name.clone();
                if name.chars().count() > 16 {
                    name = name.chars().take(15).collect::<String>() + "…";
                }
                let tile = widget::column::with_capacity(2)
                    .push(picture)
                    .push(widget::text::caption(name))
                    .spacing(3)
                    .align_x(Alignment::Center)
                    .width(Length::Fixed(108.0));
                tiles.push(
                    widget::tooltip(
                        widget::button::custom(tile)
                            .padding(2)
                            .class(cosmic::theme::Button::Image)
                            .on_press(Message::PickerChoose(entry.path.clone())),
                        widget::text::caption(entry.name.clone()),
                        widget::tooltip::Position::Bottom,
                    )
                    .into(),
                );
            }
            col = col.push(widget::flex_row(tiles).spacing(6));
            return self.hover_scroll(ScrollArea::Picker, widget::scrollable(col));
        }

        let mut list = widget::list_column();
        if let Some(parent) = self.picker_dir.parent() {
            list = list.add(
                widget::list::button(
                    widget::row::with_capacity(2)
                        .push(icon::from_name("go-up-symbolic").size(16))
                        .push(widget::text::body(".."))
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .on_press(Message::PickerNavigate(parent.to_owned())),
            );
        }
        for entry in &self.picker_entries {
            let icon_name = if entry.is_dir {
                "folder-symbolic"
            } else if images::is_image_file(&entry.path) {
                "image-x-generic-symbolic"
            } else {
                "text-x-generic-symbolic"
            };
            let msg = if entry.is_dir {
                Message::PickerNavigate(entry.path.clone())
            } else {
                Message::PickerChoose(entry.path.clone())
            };
            list = list.add(
                widget::list::button(
                    widget::row::with_capacity(2)
                        .push(icon::from_name(icon_name).size(16))
                        .push(widget::text::body(entry.name.clone()))
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .on_press(msg),
            );
        }
        col = col.push(list);
        self.hover_scroll(ScrollArea::Picker, widget::scrollable(col))
    }

    // ----- shortcuts -----

    /// The key binding shown for an action, if any ("Ctrl + B").
    fn shortcut_for(&self, action: MenuAction) -> Option<String> {
        let mut found: Vec<String> = self
            .key_binds
            .iter()
            .filter(|(_, a)| **a == action)
            .map(|(k, _)| k.to_string())
            .collect();
        // Shortest first; ties broken alphabetically so an action with
        // several spellings always shows the same one.
        found.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        found.into_iter().next()
    }

    /// "Label  ·  Ctrl + B" for tooltips.
    fn with_shortcut(&self, label: String, action: MenuAction) -> String {
        match self.shortcut_for(action) {
            Some(key) => format!("{label}  ·  {key}"),
            None => label,
        }
    }

    /// Everything with a shortcut, grouped the way the menus are.
    fn shortcut_table(&self) -> Vec<(String, Vec<(String, MenuAction)>)> {
        let format: Vec<(String, MenuAction)> = Format::ALL
            .into_iter()
            .map(|f| (f.label(), MenuAction::Format(f)))
            .collect();
        vec![
            (
                fl!("file"),
                vec![
                    (fl!("new-note"), MenuAction::NewNote),
                    (fl!("new-folder"), MenuAction::NewFolder),
                    (fl!("dock-image"), MenuAction::AddImage),
                    (fl!("dock-attach"), MenuAction::AttachFile),
                    (fl!("search-notes"), MenuAction::FocusSearch),
                    (fl!("pin-note"), MenuAction::Pin),
                    (fl!("trash-note"), MenuAction::TrashNote),
                ],
            ),
            (
                fl!("edit"),
                vec![
                    (fl!("undo"), MenuAction::Undo),
                    (fl!("redo"), MenuAction::Redo),
                ],
            ),
            (fl!("format"), format),
            (
                fl!("view"),
                vec![
                    (fl!("show-nav"), MenuAction::ToggleNav),
                    (fl!("show-list"), MenuAction::ToggleList),
                    (fl!("editor-only"), MenuAction::Solo),
                    (fl!("zoom-in"), MenuAction::ZoomIn),
                    (fl!("zoom-out"), MenuAction::ZoomOut),
                    (fl!("zoom-reset"), MenuAction::ZoomReset),
                    (fl!("theme-colours"), MenuAction::Themes),
                    (fl!("show-markers"), MenuAction::ToggleMarkers),
                    (fl!("shortcuts"), MenuAction::Shortcuts),
                ],
            ),
        ]
    }

    /// Full-window overlay listing every shortcut; × or Esc closes it.
    fn shortcuts_overlay<'a>(&'a self, p: &Palette) -> Element<'a, Message> {
        let mut columns = widget::row::with_capacity(2)
            .spacing(28)
            .align_y(Alignment::Start);
        let groups = self.shortcut_table();
        let half = groups.len().div_ceil(2);
        for chunk in groups.chunks(half) {
            let mut col = widget::column::with_capacity(24)
                .spacing(3)
                .width(Length::Fixed(300.0));
            for (group, entries) in chunk {
                col = col.push(
                    widget::container(retro::title(p, group.clone()).size(20))
                        .padding([8, 0, 2, 0]),
                );
                for (label, action) in entries {
                    let key = self.shortcut_for(*action).unwrap_or_else(|| "—".to_owned());
                    col = col.push(
                        widget::row::with_capacity(2)
                            .push(retro::text(p, label.clone()).width(Length::Fill))
                            .push(retro::accent(p, key))
                            .spacing(12),
                    );
                }
            }
            columns = columns.push(col);
        }
        let close = widget::button::custom(retro::accent(p, "×").size(18))
            .padding([0, 8])
            .class(retro::row_class(p, false))
            .on_press(Message::ToggleShortcuts);
        let header = widget::row::with_capacity(2)
            .push(retro::dim(p, fl!("shortcuts-hint")).width(Length::Fill))
            .push(close)
            .align_y(Alignment::Center);
        let body = widget::column::with_capacity(2)
            .push(header)
            .push(columns)
            .spacing(6)
            .padding([2, 6, 6, 6]);
        let card = retro::frame_sized(
            p,
            fl!("shortcuts").to_lowercase(),
            self.shortcut_for(MenuAction::Shortcuts),
            body,
            Length::Shrink,
            21.0,
        );
        let backdrop = p.bg.scale_alpha(0.88);
        widget::container(widget::container(card).max_width(700.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .padding(24)
            .class(cosmic::theme::Container::custom(move |_| {
                widget::container::Style {
                    background: Some(cosmic::iced::Background::Color(backdrop)),
                    ..Default::default()
                }
            }))
            .into()
    }

    // ----- blocks & images -----

    /// Focus the text block that currently has the caret.
    /// Write the open cell draft into its table, if any.
    fn commit_table_edit(&mut self) {
        if let Some((block, row, col)) = self.table_edit.take() {
            let draft = std::mem::take(&mut self.table_draft);
            if self
                .blocks
                .table_mut(block)
                .is_some_and(|t| t.cell(row, col) != draft)
            {
                self.record(EditKind::Other);
                if let Some(t) = self.blocks.table_mut(block) {
                    t.set_cell(row, col, draft);
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            }
        }
    }

    fn focus_editor(&self) -> Task<cosmic::Action<Message>> {
        // Through the shared Content, not a focus operation: an id-targeted
        // operation unfocuses every non-matching widget, and a stale id
        // (blocks rebuild, ids drift) turned that into the focused editor
        // losing its own selection after every dock/menu format action.
        // Every other editor lets go — exactly one holds the keys.
        for (i, block) in self.blocks.items.iter().enumerate() {
            if let Block::Text { content, .. } = block {
                if i == self.blocks.focused {
                    content.request_focus();
                } else {
                    content.request_unfocus();
                }
            }
        }
        Task::none()
    }

    /// No editor holds the keyboard: a table (cell input or selection) does.
    fn blur_editors(&self) {
        for block in &self.blocks.items {
            if let Block::Text { content, .. } = block {
                content.request_unfocus();
            }
        }
    }

    /// The note body as a column of text editors and inline images.
    fn blocks_view<'a>(&'a self, p: &Palette, trashed: bool) -> Element<'a, Message> {
        let items = &self.blocks.items;
        let last_text = self.blocks.last_text();
        // While a picture is being dragged the note becomes a drop target:
        // plain lines with hover slots instead of editors.
        let drag = self.dragging.filter(|d| d.active);
        let offsets = self.blocks.line_offsets();
        let target = drag.and_then(|d| d.target);
        // A file dragged in from outside: the text widgets draw the line where
        // it lands; images and rules get the same indicator from here.
        let drop = self.drop_hover.then_some(self.drop_target).flatten();
        let mut drop_claimed = false;
        for (i, block) in items.iter().enumerate() {
            if let Block::Text { content, .. } = block {
                let owns =
                    drop.is_some_and(|t| !drop_claimed && t >= offsets[i] && t <= offsets[i + 1]);
                if owns {
                    drop_claimed = true;
                    content.set_drop_marker(drop.map(|t| t - offsets[i]));
                } else {
                    content.set_drop_marker(None);
                }
            }
        }
        let text_el = |i: usize, content: &'a crate::editor::Content, id: widget::Id| {
            if drag.is_some() {
                self.drag_lines(p, content, offsets[i], target, i == last_text)
            } else {
                self.text_block(p, i, content, id, trashed, i == last_text)
            }
        };
        let mut col = widget::column::with_capacity(items.len() + 1)
            .spacing(6)
            .width(Length::Fill);
        let mut i = 0;
        while i < items.len() {
            match &items[i] {
                Block::Text { content, id } => {
                    col = col.push(text_el(i, content, id.clone()));
                    i += 1;
                }
                Block::Table(t) => {
                    col = col.push(self.table_el(p, i, t));
                    i += 1;
                }
                Block::Rule(_) => {
                    let rule = retro::rule_block(p, self.rule_size.rule_px());
                    if drag.is_some() {
                        let slot = |line: usize| {
                            widget::mouse_area(
                                widget::Space::new()
                                    .width(Length::Fill)
                                    .height(Length::Fill),
                            )
                            .on_move(move |_| Message::DragOver(line))
                            .interaction(mouse::Interaction::Grabbing)
                        };
                        let halves = widget::column::with_capacity(2)
                            .push(slot(offsets[i]))
                            .push(slot(offsets[i] + 1))
                            .width(Length::Fill)
                            .height(Length::Fill);
                        col = col.push(crate::probe::probe(
                            cosmic::iced::widget::stack([rule, halves.into()]).width(Length::Fill),
                            self.note_cell(offsets[i]),
                        ));
                    } else {
                        col = col.push(rule);
                    }
                    i += 1;
                }
                Block::Link(l) => {
                    let card = self.link_card(p, l, i);
                    if drag.is_some() {
                        let slot = |line: usize| {
                            widget::mouse_area(
                                widget::Space::new()
                                    .width(Length::Fill)
                                    .height(Length::Fill),
                            )
                            .on_move(move |_| Message::DragOver(line))
                            .interaction(mouse::Interaction::Grabbing)
                        };
                        let halves = widget::column::with_capacity(2)
                            .push(slot(offsets[i]))
                            .push(slot(offsets[i] + 1))
                            .width(Length::Fill)
                            .height(Length::Fill);
                        col = col.push(crate::probe::probe(
                            cosmic::iced::widget::stack([card, halves.into()]).width(Length::Fill),
                            self.note_cell(offsets[i]),
                        ));
                    } else {
                        col = col.push(card);
                    }
                    i += 1;
                }
                Block::Image(r) => {
                    let card = match drag {
                        Some(d) => self.drag_image_card(p, r, i, offsets[i], d.block == i),
                        None => self.image_card(p, r, i),
                    };
                    match r.align {
                        Align::Center => {
                            col = col.push(crate::probe::probe(
                                widget::container(card)
                                    .width(Length::Fill)
                                    .align_x(Alignment::Center)
                                    .padding([4, 10]),
                                self.note_cell(offsets[i]),
                            ));
                            i += 1;
                        }
                        Align::Left | Align::Right => {
                            let paired = match items.get(i + 1) {
                                Some(Block::Text { content, id }) => {
                                    Some(text_el(i + 1, content, id.clone()))
                                }
                                _ => None,
                            };
                            let has_pair = paired.is_some();
                            let card = widget::container(card).padding([4, 10]);
                            let row = match (r.align, paired) {
                                (Align::Left, Some(text)) => {
                                    widget::row::with_capacity(2).push(card).push(text)
                                }
                                (_, Some(text)) => {
                                    widget::row::with_capacity(2).push(text).push(card)
                                }
                                (Align::Left, None) => widget::row::with_capacity(1).push(card),
                                (_, None) => widget::row::with_capacity(2)
                                    .push(widget::Space::new().width(Length::Fill))
                                    .push(card),
                            };
                            col = col.push(crate::probe::probe(
                                row.spacing(8).align_y(Alignment::Start).width(Length::Fill),
                                self.note_cell(offsets[i]),
                            ));
                            i += if has_pair { 2 } else { 1 };
                        }
                    }
                }
            }
        }
        // The drop indicator floats over the rows at its gliding y instead
        // of being spliced in, so nothing shifts while it moves.
        let col: Element<'a, Message> =
            crate::probe::probe(col.width(Length::Fill), self.note_area.clone()).into();
        let col: Element<'a, Message> = if drag.is_some_and(|d| d.target.is_some())
            && self.note_line.shown()
        {
            let line = widget::container(retro::drop_line(p, fl!("drop-here")))
                .padding(cosmic::iced::Padding {
                    top: self.note_line.pos.max(0.0),
                    ..Default::default()
                })
                .width(Length::Fill);
            cosmic::iced::widget::stack([col, line.into()])
                .width(Length::Fill)
                .into()
        } else {
            col
        };
        // The measure: the column never grows past the chosen width and sits
        // centred with margins; a narrower pane simply wraps the text.
        let mut column = widget::container(col).width(Length::Fill);
        if let Some(max) = self.measure.max_width() {
            column = column.max_width(max);
        }
        let col = widget::container(column)
            .width(Length::Fill)
            .align_x(Alignment::Center);
        self.hover_scroll(
            ScrollArea::Editor,
            widget::scrollable(col)
                .id(self.editor_scroll_id.clone())
                .height(Length::Fill)
                .width(Length::Fill),
        )
    }

    fn text_block<'a>(
        &'a self,
        p: &Palette,
        block: usize,
        content: &'a crate::editor::Content,
        id: widget::Id,
        trashed: bool,
        is_last: bool,
    ) -> Element<'a, Message> {
        let settings = markdown::Settings {
            palette: *p,
            show_markers: self.show_markers,
            font: self.body_font(),
            tag_icons: Arc::clone(&self.tag_icons),
            icon_set: self.icon_set,
        };
        let mut editor = crate::editor::RichEditor::new(content, settings)
            .drop_anim(crate::anim::params(self.anim_ms, self.anim_ease10))
            .id(id)
            .placeholder(if block == 0 {
                fl!("untitled")
            } else {
                String::new()
            })
            .size(f32::from(self.font_size))
            .padding([6, 10])
            .drop_label(fl!("drop-here"));
        if is_last {
            editor = editor.min_height(220.0);
        }
        if !trashed {
            editor = editor
                .on_action(move |a| Message::Editor(block, a))
                .on_link(Message::FollowLink)
                .on_caret(Message::RevealCaret)
                .on_mark(|| Message::MarkSweep);
        }
        editor.into()
    }

    /// One table block: a grid of clickable cells, drag handles on every
    /// column and row edge, and a small toolbar while a cell is open.
    fn table_el<'a>(
        &'a self,
        p: &Palette,
        block: usize,
        t: &'a crate::table::Table,
    ) -> Element<'a, Message> {
        let editing_here = self
            .table_edit
            .filter(|(b, _, _)| *b == block)
            .map(|(_, r, c)| (r, c));
        // Formula pointing: while the draft wants a reference, cells are
        // targets, and the swept range lights up. A pick in flight keeps
        // the mode on — the ref just written ends the draft with a digit,
        // which must not switch the cells back mid-sweep.
        let pointing = editing_here.is_some()
            && (ref_expected(&self.table_draft)
                || self.table_pick.as_ref().is_some_and(|p| p.block == block));
        let picked = self
            .table_pick
            .as_ref()
            .filter(|p| p.block == block)
            .map(|p| {
                (
                    p.anchor.0.min(p.current.0)..=p.anchor.0.max(p.current.0),
                    p.anchor.1.min(p.current.1)..=p.anchor.1.max(p.current.1),
                )
            })
            .or_else(|| {
                self.table_fill
                    .filter(|f| f.block == block)
                    .and_then(fill_targets)
            })
            .or_else(|| {
                self.table_sel
                    .filter(|s| s.block == block && s.active)
                    .map(|s| {
                        (
                            s.anchor.0.min(s.current.0)..=s.anchor.0.max(s.current.0),
                            s.anchor.1.min(s.current.1)..=s.anchor.1.max(s.current.1),
                        )
                    })
            });
        let filling = self.table_fill.is_some_and(|f| f.block == block);
        let sel_drag = self.table_sel.is_some_and(|s| s.block == block);
        let cols = t.cols();
        let line = |w: f32, h: f32, colour: cosmic::iced::Color| {
            widget::container(widget::Space::new())
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .class(cosmic::theme::Container::custom(move |_| {
                    widget::container::Style {
                        background: Some(cosmic::iced::Background::Color(colour)),
                        ..Default::default()
                    }
                }))
        };
        let mut grid = widget::column::with_capacity(t.rows.len() * 2 + 1);
        for r in 0..t.rows.len() {
            let row_h = if t.heights[r] > 0.0 { t.heights[r] } else { 32.0 };
            let mut cells = widget::row::with_capacity(cols * 2);
            for c in 0..cols {
                let is_edit = editing_here == Some((r, c));
                let width = t.widths[c];
                let inner: Element<'_, Message> = if is_edit {
                    widget::text_input("", &self.table_draft)
                        .id(self.table_input.clone())
                        .on_input(Message::TableDraft)
                        .on_submit(|_| Message::TableCommit(TableMove::Down))
                        .style(retro::search_class(p))
                        .into()
                } else {
                    let raw = t.cell(r, c);
                    let shown = t.display(r, c);
                    let text = if r == 0 {
                        retro::accent(p, shown)
                    } else if shown == "#ERR" {
                        retro::accent(p, shown)
                    } else if raw.starts_with('=') {
                        retro::accent2(p, shown)
                    } else {
                        retro::text(p, shown)
                    };
                    text.into()
                };
                let in_pick = picked
                    .as_ref()
                    .is_some_and(|(rs, cs)| rs.contains(&r) && cs.contains(&c));
                let sel = p.sel;
                let mut cell = widget::container(inner)
                    .padding([2, 8])
                    .align_y(Alignment::Center)
                    .align_x(match t.align[c] {
                        crate::table::Align::Left => Alignment::Start,
                        crate::table::Align::Center => Alignment::Center,
                        crate::table::Align::Right => Alignment::End,
                    })
                    .height(Length::Fixed(row_h))
                    .width(if width > 0.0 {
                        Length::Fixed(width)
                    } else {
                        Length::Fill
                    });
                if in_pick {
                    cell = cell.class(cosmic::theme::Container::custom(move |_| {
                        widget::container::Style {
                            background: Some(cosmic::iced::Background::Color(sel)),
                            ..Default::default()
                        }
                    }));
                }
                if is_edit {
                    // The fill handle: Numbers' little corner dot — drag it
                    // to replicate this cell down or across, formulas
                    // translated to suit each row.
                    let accent = p.accent;
                    let dot = widget::mouse_area(
                        widget::container(widget::Space::new())
                            .width(Length::Fixed(9.0))
                            .height(Length::Fixed(9.0))
                            .class(cosmic::theme::Container::custom(move |_| {
                                widget::container::Style {
                                    background: Some(cosmic::iced::Background::Color(accent)),
                                    border: cosmic::iced::Border {
                                        radius: 2.0.into(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }
                            })),
                    )
                    .on_press(Message::TableFillStart(block, r, c))
                    .interaction(mouse::Interaction::Crosshair);
                    cells = cells.push(cosmic::iced::widget::stack([
                        cell.into(),
                        widget::container(dot)
                            .align_x(Alignment::End)
                            .align_y(Alignment::End)
                            .width(if width > 0.0 {
                                Length::Fixed(width)
                            } else {
                                Length::Fill
                            })
                            .height(Length::Fixed(row_h))
                            .into(),
                    ]));
                } else if filling {
                    cells = cells.push(
                        widget::mouse_area(cell)
                            .on_move(move |_| Message::TableFillOver(block, r, c))
                            .interaction(mouse::Interaction::Crosshair),
                    );
                } else if pointing {
                    let mut area = widget::mouse_area(cell)
                        .on_press(Message::TablePick(block, r, c))
                        .interaction(mouse::Interaction::Crosshair);
                    if self.table_pick.is_some() {
                        area = area.on_move(move |_| Message::TablePickOver(block, r, c));
                    }
                    cells = cells.push(area);
                } else {
                    // Press-and-release opens the cell; press-and-sweep
                    // rubber-bands a selection for Delete to clear.
                    let mut area = widget::mouse_area(cell)
                        .on_press(Message::TableSelStart(block, r, c))
                        .interaction(mouse::Interaction::Text);
                    if sel_drag {
                        area = area.on_move(move |_| Message::TableSelOver(block, r, c));
                    }
                    cells = cells.push(area);
                }
                if c + 1 < cols {
                    // The column edge: a hairline with a wider grab zone.
                    let est = if width > 0.0 { width } else { 140.0 };
                    cells = cells.push(
                        widget::mouse_area(
                            widget::container(line(1.0, row_h, p.border))
                                .width(Length::Fixed(7.0))
                                .align_x(Alignment::Center),
                        )
                        .on_press(Message::TableResizeStart(block, true, c, est))
                        .interaction(mouse::Interaction::ResizingHorizontally),
                    );
                }
            }
            grid = grid.push(cells.align_y(Alignment::Center));
            // The row edge: under the header it doubles as the head rule.
            let colour = if r == 0 { p.dim } else { p.border };
            grid = grid.push(
                widget::mouse_area(
                    widget::container(
                        widget::container(widget::Space::new())
                            .width(Length::Fill)
                            .height(Length::Fixed(1.0))
                            .class(cosmic::theme::Container::custom(move |_| {
                                widget::container::Style {
                                    background: Some(cosmic::iced::Background::Color(colour)),
                                    ..Default::default()
                                }
                            })),
                    )
                    .height(Length::Fixed(5.0))
                    .align_y(Alignment::Center),
                )
                .on_press(Message::TableResizeStart(block, false, r, row_h))
                .interaction(mouse::Interaction::ResizingVertically),
            );
        }
        let mut col = widget::column::with_capacity(3).spacing(2);
        if editing_here.is_some() && !self.config.table_toolbar_off {
            // Labels only; each explains itself on hover.
            let tool = |label: String, hint: String, msg: Message| {
                widget::tooltip(
                    widget::button::custom(retro::dim(p, label))
                        .padding([2, 8])
                        .class(cosmic::theme::Button::Text)
                        .on_press(msg),
                    widget::text::body(hint),
                    widget::tooltip::Position::Top,
                )
            };
            col = col.push(
                widget::row::with_capacity(6)
                    .push(tool(
                        fl!("table-add-row"),
                        fl!("table-add-row-hint"),
                        Message::TableAddRow(block),
                    ))
                    .push(tool(
                        fl!("table-add-col"),
                        fl!("table-add-col-hint"),
                        Message::TableAddCol(block),
                    ))
                    .push(tool(
                        fl!("table-del-row"),
                        fl!("table-del-row-hint"),
                        Message::TableRemoveRow(block),
                    ))
                    .push(tool(
                        fl!("table-del-col"),
                        fl!("table-del-col-hint"),
                        Message::TableRemoveCol(block),
                    ))
                    .push(tool(
                        fl!("table-money"),
                        fl!("table-money-hint"),
                        Message::TableMoney,
                    ))
                    .push(
                        widget::tooltip(
                            widget::container(retro::dim(p, fl!("table-fx")))
                                .padding([2, 8])
                                .align_y(Alignment::Center),
                            widget::text::body(fl!("table-hint")),
                            widget::tooltip::Position::Top,
                        ),
                    )
                    .spacing(2)
                    .align_y(Alignment::Center),
            );
        }
        col = col.push(retro::bordered(p, grid.into()));
        widget::container(col).padding([4, 0]).into()
    }

    fn image_key(&self, r: &ImageRef) -> Option<(String, PathBuf)> {
        let store = self.store.as_ref()?;
        let path = images::resolve(store.notes_dir(), &r.path);
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let theme = if r.frame.themed() {
            self.palette_id()
        } else {
            String::new()
        };
        let cols = if r.frame == FrameStyle::Ascii {
            images::ascii_layout(r.width).0
        } else {
            0
        };
        Some((
            format!(
                "{}|{mtime}|{}|{theme}|{cols}",
                path.display(),
                r.frame.key()
            ),
            path,
        ))
    }

    /// Spawn loads for every referenced image not yet in the cache.
    fn image_loads(&mut self) -> Vec<Task<cosmic::Action<Message>>> {
        let Some(note) = &self.current else {
            return Vec::new();
        };
        if note.trashed {
            return Vec::new();
        }
        let palette = self.palette();
        let refs: Vec<ImageRef> = self
            .blocks
            .images()
            .into_iter()
            .map(|(_, r)| r.clone())
            .collect();
        let mut tasks = Vec::new();
        for r in refs {
            let Some((key, path)) = self.image_key(&r) else {
                continue;
            };
            if self.image_cache.contains_key(&key) {
                continue;
            }
            self.image_cache.insert(key.clone(), ImageState::Loading);
            let style = r.frame;
            let cols = images::ascii_layout(r.width).0;
            tasks.push(Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        images::load_and_process(&path, style, images::inks(&palette), cols)
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| format!("{e:#}")))
                },
                move |result| cosmic::Action::App(Message::ImageLoaded(key.clone(), result)),
            ));
        }
        tasks
    }

    /// Copy a file into assets/ and drop it into the note at the caret.
    fn import_image(&mut self, path: &std::path::Path) -> Task<cosmic::Action<Message>> {
        self.import_image_at(path, None)
    }

    /// Decode thumbnails for the picker's images that are not cached yet.
    fn load_thumbs(&mut self) -> Task<cosmic::Action<Message>> {
        let mut tasks = Vec::new();
        for entry in self
            .picker_entries
            .iter()
            .filter(|e| !e.is_dir && images::is_image_file(&e.path))
        {
            if self.thumbs.contains_key(&entry.path) {
                continue;
            }
            self.thumbs.insert(entry.path.clone(), ImageState::Loading);
            let path = entry.path.clone();
            tasks.push(Task::perform(
                async move {
                    let p = path.clone();
                    let result = tokio::task::spawn_blocking(move || images::thumbnail(&p, 160))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| format!("{e:#}")));
                    (path, result)
                },
                |(path, result)| cosmic::Action::App(Message::ThumbLoaded(path, result)),
            ));
        }
        Task::batch(tasks)
    }

    /// Body line a drag would drop before. The offer's own coordinates
    /// (`_x`, `_y`) are window-relative and ignore scrolling, so the pointer
    /// position is read back from the editors instead: the drag is forwarded
    /// to them as cursor motion, which the scroll container translates into
    /// the same space as their recorded bounds.
    fn drop_line_at(&self, _x: f32, _y: f32) -> usize {
        let offsets = self.blocks.line_offsets();
        let pointer = self.blocks.items.iter().find_map(|b| match b {
            Block::Text { content, .. } => content.pointer_y(),
            _ => None,
        });
        let Some(y) = pointer else {
            return self.drop_target.unwrap_or(0);
        };
        for (i, block) in self.blocks.items.iter().enumerate() {
            let Block::Text { content, .. } = block else {
                continue;
            };
            let b = content.bounds();
            if b.height <= 0.0 {
                continue;
            }
            if y < b.y {
                return offsets[i];
            }
            if y < b.y + b.height {
                let (line, after) = content.line_at_y(y - b.y - 6.0);
                let lines = offsets[i + 1] - offsets[i];
                return (offsets[i] + line + usize::from(after)).min(offsets[i] + lines);
            }
        }
        *offsets.last().unwrap_or(&0)
    }

    /// Import an image and place it before body line `target` (or at the
    /// caret when `None`).
    fn import_image_at(
        &mut self,
        path: &std::path::Path,
        target: Option<usize>,
    ) -> Task<cosmic::Action<Message>> {
        if self.current.as_ref().is_none_or(|n| n.trashed) {
            return Task::none();
        }
        let Some(store) = &self.store else {
            return Task::none();
        };
        let rel = match images::import_asset(store.notes_dir(), path) {
            Ok(rel) => rel,
            Err(err) => {
                tracing::error!(%err, "importing image");
                return Task::none();
            }
        };
        let alt = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .to_owned();
        // New pictures arrive plain, across the text column; the retro
        // frames are a choice made per picture in its menu. (`Box` stays
        // the format's default so bare lines in older notes keep theirs.)
        let r = ImageRef {
            line: 0,
            alt,
            path: rel,
            frame: FrameStyle::Plain,
            align: Align::default(),
            width: None,
        };
        self.record(EditKind::Other);
        let next = self.blocks.insert_image(r);
        if let Some(target) = target
            && let Some(block) = next.checked_sub(1)
        {
            // The insert added a line at the caret; keep the target honest.
            let img_line = self.blocks.line_offsets()[block];
            let target = if img_line < target {
                target + 1
            } else {
                target
            };
            self.blocks.move_image(block, target);
        }
        self.dirty = true;
        self.last_edit = Instant::now();
        self.focus_editor()
    }

    /// Attach any file: copy it into `assets/` and drop a link card before
    /// body line `target` (or at the caret).
    fn import_file_at(
        &mut self,
        path: &std::path::Path,
        target: Option<usize>,
    ) -> Task<cosmic::Action<Message>> {
        if self.current.as_ref().is_none_or(|n| n.trashed) {
            return Task::none();
        }
        let Some(store) = &self.store else {
            return Task::none();
        };
        let rel = match images::import_asset(store.notes_dir(), path) {
            Ok(rel) => rel,
            Err(err) => {
                tracing::error!(%err, "attaching file");
                return Task::none();
            }
        };
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .replace(['[', ']'], "");
        let l = crate::links::LinkRef {
            text: name,
            target: rel,
        };
        self.record(EditKind::Other);
        let next = self.blocks.insert_link(l);
        if let Some(target) = target
            && let Some(block) = next.checked_sub(1)
        {
            let line = self.blocks.line_offsets()[block];
            let target = if line < target { target + 1 } else { target };
            self.blocks.move_image(block, target);
        }
        self.dirty = true;
        self.last_edit = Instant::now();
        self.focus_editor()
    }

    /// Start fetching previews for web links that have none yet (cached
    /// ones load at once). No-op when previews are switched off.
    fn queue_previews(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(store) = &self.store else {
            return Task::none();
        };
        let notes_dir = store.notes_dir().to_path_buf();
        let mut tasks = Vec::new();
        let urls: Vec<String> = self
            .blocks
            .links()
            .into_iter()
            .filter(|(_, l)| l.is_web())
            .map(|(_, l)| l.target.clone())
            .collect();
        for url in urls {
            if self.previews.contains_key(&url) {
                continue;
            }
            if let Some(p) = crate::links::load_cached(&notes_dir, &url) {
                let handle = p.image.as_ref().map(widget::image::Handle::from_path);
                self.previews.insert(url, PreviewState::Ready(p, handle));
                continue;
            }
            if !self.link_previews {
                continue;
            }
            self.previews.insert(url.clone(), PreviewState::Loading);
            let dir = notes_dir.clone();
            let key = url.clone();
            tasks.push(Task::perform(
                async move {
                    let u = url.clone();
                    tokio::task::spawn_blocking(move || crate::links::fetch(&dir, &u))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| e.to_string()))
                },
                move |result| cosmic::Action::App(Message::PreviewLoaded(key.clone(), result)),
            ));
        }
        Task::batch(tasks)
    }

    /// A link card: web page (picture, title, description, address) or
    /// attached file (kind, name, size). Click to open, ⋯ for the menu.
    fn link_card<'a>(
        &'a self,
        p: &Palette,
        l: &'a crate::links::LinkRef,
        block: usize,
    ) -> Element<'a, Message> {
        let font = self.body_font();
        let size = f32::from(self.font_size);
        let menu_open = self.link_menu == Some(block);
        let editable = self.current.as_ref().is_some_and(|n| !n.trashed);

        let body: Element<'a, Message> = if l.is_web() {
            let state = self.previews.get(&l.target);
            let (title, desc, handle) = match state {
                Some(PreviewState::Ready(pv, handle)) => (
                    if pv.title.is_empty() {
                        l.label()
                    } else {
                        pv.title.clone()
                    },
                    pv.description.clone(),
                    handle.clone(),
                ),
                Some(PreviewState::Loading) => (l.label(), fl!("link-fetching"), None),
                Some(PreviewState::Failed(err)) => (
                    l.label(),
                    format!(
                        "{} — {}",
                        fl!("link-offline"),
                        crate::links::truncate(err, 70)
                    ),
                    None,
                ),
                None => (l.label(), String::new(), None),
            };
            let mut text = widget::column::with_capacity(3)
                .spacing(3)
                .width(Length::Fill);
            text = text.push(
                retro::text(p, crate::links::truncate(&title, 90))
                    .font(font)
                    .size(size)
                    .wrapping(cosmic::iced::widget::text::Wrapping::Word),
            );
            if !desc.is_empty() {
                text = text.push(
                    retro::dim(p, crate::links::truncate(&desc, 140))
                        .font(font)
                        .size(size * 0.86),
                );
            }
            text = text.push(retro::accent2(p, format!("⌁ {}", l.short())).size(size * 0.8));
            let mut row = widget::row::with_capacity(2)
                .spacing(12)
                .align_y(Alignment::Center)
                .width(Length::Fill);
            if let Some(h) = handle {
                row = row.push(
                    widget::image(h)
                        .width(Length::Fixed(112.0))
                        .height(Length::Fixed(80.0))
                        .content_fit(cosmic::iced::ContentFit::Cover),
                );
            }
            row.push(text).into()
        } else {
            let path = self
                .store
                .as_ref()
                .map(|s| images::resolve(s.notes_dir(), &l.target))
                .unwrap_or_else(|| std::path::PathBuf::from(&l.target));
            let info = crate::links::file_info(&path);
            let badge = widget::container(retro::accent(p, info.kind.clone()).size(size * 0.8))
                .padding([10, 6])
                .width(Length::Fixed(56.0))
                .height(Length::Fixed(64.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .class(retro::dock_class(p));
            let mut text = widget::column::with_capacity(3)
                .spacing(3)
                .width(Length::Fill);
            text = text.push(retro::text(p, l.label()).font(font).size(size));
            text = text.push(
                retro::dim(p, crate::links::kind_label(&info.kind))
                    .font(font)
                    .size(size * 0.86),
            );
            text = text.push(
                retro::accent2(
                    p,
                    if info.exists {
                        info.size.clone()
                    } else {
                        fl!("file-missing")
                    },
                )
                .size(size * 0.8),
            );
            widget::row::with_capacity(2)
                .push(badge)
                .push(text)
                .spacing(12)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into()
        };
        let open_msg = if l.is_web() {
            Message::LaunchUrl(l.target.clone())
        } else {
            Message::OpenFile(l.target.clone())
        };
        // Double-click opens the target; press and drag moves the card, the
        // same way a picture moves.
        let clickable =
            widget::mouse_area(widget::container(body).padding([8, 12]).width(Length::Fill))
                .on_press(Message::LinkPress(block))
                .on_double_click(open_msg)
                .interaction(if editable {
                    mouse::Interaction::Grab
                } else {
                    mouse::Interaction::Pointer
                });
        let dots = widget::container(
            widget::button::custom(retro::accent(p, "⋯").size(14))
                .padding([0, 6])
                .class(retro::row_class(p, menu_open))
                .on_press_maybe(editable.then_some(Message::LinkMenu(Some(block)))),
        )
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding([4, 4, 0, 0]);
        let stacked =
            cosmic::iced::widget::stack([clickable.into(), dots.into()]).width(Length::Fill);
        let frame = if self.card_border {
            self.card_line.card_px()
        } else {
            0.0
        };
        let card = widget::container(retro::lifted(p, stacked.into(), frame))
            .width(Length::Fill)
            .padding([2, 10]);
        if menu_open {
            widget::popover(card)
                .popup(self.link_menu_view(p, l, block))
                .position(widget::popover::Position::Bottom)
                .on_close(Message::LinkMenu(None))
                .into()
        } else {
            card.into()
        }
    }

    /// The ⋯ menu of a link card: open, copy, refresh, remove.
    fn link_menu_view<'a>(
        &'a self,
        p: &Palette,
        l: &'a crate::links::LinkRef,
        block: usize,
    ) -> Element<'a, Message> {
        let item = |label: String, msg: Message| {
            widget::button::custom(retro::text(p, label))
                .padding([2, 8])
                .width(Length::Fill)
                .class(retro::row_class(p, false))
                .on_press(msg)
        };
        let mut col = widget::column::with_capacity(5)
            .spacing(1)
            .width(Length::Fixed(180.0));
        if l.is_web() {
            col = col.push(item(fl!("link-open"), Message::LaunchUrl(l.target.clone())));
            col = col.push(item(fl!("link-copy"), Message::CopyLink(l.target.clone())));
            if self.link_previews {
                col = col.push(item(
                    fl!("link-refresh"),
                    Message::RefreshPreview(l.target.clone()),
                ));
            }
        } else {
            col = col.push(item(fl!("link-open"), Message::OpenFile(l.target.clone())));
        }
        col = col.push(item(fl!("menu-remove"), Message::RemoveLink(block)));
        widget::container(col)
            .padding([6, 4])
            .class(retro::dock_class(p))
            .into()
    }

    /// Change one image's attributes in place.
    fn edit_ref(&mut self, block: usize, f: impl FnOnce(&mut ImageRef)) {
        self.edit_ref_kind(block, EditKind::Other, f);
    }

    fn edit_ref_kind(&mut self, block: usize, kind: EditKind, f: impl FnOnce(&mut ImageRef)) {
        if self.blocks.image_mut(block).is_none() {
            return;
        }
        self.record(kind);
        if let Some(r) = self.blocks.image_mut(block) {
            f(r);
            self.dirty = true;
            self.last_edit = Instant::now();
        }
    }

    // ----- undo -----

    fn snapshot(&self) -> Snapshot {
        let cursor = self
            .blocks
            .text(self.blocks.focused)
            .map(|c| c.cursor())
            .unwrap_or(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: None,
            });
        Snapshot {
            body: self.blocks.body(),
            focused: self.blocks.focused,
            cursor,
        }
    }

    /// Push an undo step before an edit, unless it continues the current
    /// typing/deleting burst. Any edit invalidates the redo stack.
    fn record(&mut self, kind: EditKind) {
        let continues = kind != EditKind::Other
            && kind == self.last_undo_kind
            && self.last_undo_at.elapsed() < UNDO_GROUP_IDLE;
        self.last_undo_at = Instant::now();
        self.last_undo_kind = kind;
        self.redo.clear();
        if continues {
            return;
        }
        let snap = self.snapshot();
        self.push_undo(snap);
    }

    /// Push a snapshot as an undo step (skipping an exact repeat).
    fn push_undo(&mut self, snap: Snapshot) {
        if self.undo.last().is_some_and(|last| last.body == snap.body) {
            return;
        }
        self.undo.push(snap);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snap: Snapshot) {
        self.blocks
            .rebuild(&snap.body, snap.focused, Some(snap.cursor));
        self.image_menu = None;
        self.last_undo_kind = EditKind::Other;
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// A text block while a picture is being dragged: one plain line per body
    /// line, each split into a top half (drop above) and a bottom half (drop
    /// below), with the drop line drawn exactly where the picture will land.
    fn drag_lines<'a>(
        &'a self,
        p: &Palette,
        content: &'a crate::editor::Content,
        offset: usize,
        _target: Option<usize>,
        is_last: bool,
    ) -> Element<'a, Message> {
        let slot = |line: usize, height: Length| {
            widget::mouse_area(widget::Space::new().width(Length::Fill).height(height))
                .on_move(move |_| Message::DragOver(line))
                .interaction(mouse::Interaction::Grabbing)
        };
        let text = content.text();
        let text = text.strip_suffix('\n').unwrap_or(&text);
        let mut col =
            widget::column::with_capacity(text.lines().count() * 2 + 2).width(Length::Fill);
        let count = if text.is_empty() {
            // A gap between pictures owns no line; hovering it means "here".
            col = col.push(crate::probe::probe(
                slot(offset, Length::Fixed(if is_last { 120.0 } else { 14.0 })),
                self.note_cell(offset),
            ));
            0
        } else {
            let mut count = 0;
            for (k, line) in text.split('\n').enumerate() {
                let shown = if line.is_empty() {
                    " ".to_owned()
                } else {
                    line.to_owned()
                };
                let line_el = retro::text(p, shown)
                    .font(self.body_font())
                    .size(f32::from(self.font_size))
                    .line_height(1.5)
                    .width(Length::Fill);
                let halves = widget::column::with_capacity(2)
                    .push(slot(offset + k, Length::Fill))
                    .push(slot(offset + k + 1, Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill);
                col = col.push(crate::probe::probe(
                    cosmic::iced::widget::stack([line_el.into(), halves.into()])
                        .width(Length::Fill),
                    self.note_cell(offset + k),
                ));
                count = k + 1;
            }
            count
        };
        if is_last && count > 0 {
            // Room below the last line so the end of the note is a target too.
            col = col.push(slot(offset + count, Length::Fixed(120.0)));
        }
        widget::container(col)
            .padding([6, 10])
            .width(Length::Fill)
            .into()
    }

    /// An image while a picture is being dragged: top half drops above it,
    /// bottom half below; the one being moved is outlined.
    fn drag_image_card<'a>(
        &'a self,
        p: &Palette,
        r: &'a ImageRef,
        block: usize,
        offset: usize,
        dragged: bool,
    ) -> Element<'a, Message> {
        let pic = self.image_picture(p, r, block);
        let slot = |line: usize| {
            widget::mouse_area(
                widget::Space::new()
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_move(move |_| Message::DragOver(line))
            .interaction(mouse::Interaction::Grabbing)
        };
        let halves = widget::column::with_capacity(2)
            .push(slot(offset))
            .push(slot(offset + 1))
            .width(Length::Fill)
            .height(Length::Fill);
        let stacked = cosmic::iced::widget::stack([pic, halves.into()]).width(Length::Fill);
        let el: Element<'a, Message> = if dragged {
            retro::lifted(p, stacked.into(), 2.0)
        } else {
            stacked.into()
        };
        self.size_card(r, block, el).into()
    }

    /// The card at its chosen width (live while resizing).
    fn size_card<'a>(
        &self,
        r: &ImageRef,
        block: usize,
        content: Element<'a, Message>,
    ) -> widget::Container<'a, Message, cosmic::Theme> {
        let width = self
            .live_width
            .filter(|(b, _)| *b == block)
            .map(|(_, w)| w)
            .or(r.width);
        match (r.align, width) {
            (Align::Center, Some(w)) => widget::container(content)
                .max_width(w as f32)
                .width(Length::Fill),
            (Align::Center, None) => widget::container(content).width(Length::Fill),
            (_, w) => widget::container(content).width(Length::Fixed(w.unwrap_or(360) as f32)),
        }
    }

    /// The picture itself in its frame treatment (or its loading / error state).
    fn image_picture<'a>(
        &'a self,
        p: &Palette,
        r: &'a ImageRef,
        block: usize,
    ) -> Element<'a, Message> {
        let state = self
            .image_key(r)
            .and_then(|(k, _)| self.image_cache.get(&k));
        let pic: Element<'a, Message> = match state {
            Some(ImageState::Ready(handle, w, h)) => {
                let (w, h) = (*w, *h);
                let img = widget::image(handle.clone())
                    .width(Length::Fill)
                    .content_fit(cosmic::iced::ContentFit::Contain);
                match r.frame {
                    FrameStyle::Plain => img.into(),
                    FrameStyle::Box => retro::frame_sized(
                        p,
                        if r.alt.trim().is_empty() {
                            r.file_name().to_owned()
                        } else {
                            r.alt.clone()
                        },
                        Some(format!("{w}×{h}")),
                        img,
                        Length::Shrink,
                        16.0,
                    ),
                    FrameStyle::Tint
                    | FrameStyle::Dither
                    | FrameStyle::Pixel
                    | FrameStyle::Ascii => retro::bordered(p, img.into()),
                    FrameStyle::Bezel => retro::bezel(p, img.into()),
                    FrameStyle::Print => retro::print(p, img.into(), r.alt.clone()),
                    FrameStyle::Film => retro::film(p, img.into(), self.image_number(block)),
                    FrameStyle::Comic => retro::comic(p, img.into(), r.alt.clone()),
                }
            }
            Some(ImageState::Ascii(text)) => {
                let live = self
                    .live_width
                    .filter(|(b, _)| *b == block)
                    .map(|(_, w)| w)
                    .or(r.width);
                retro::ascii_card(p, text.clone(), images::ascii_layout(live).1)
            }
            Some(ImageState::Failed(err)) => widget::container(retro::dim(p, format!("⚠ {err}")))
                .padding(8)
                .width(Length::Fill)
                .into(),
            Some(ImageState::Loading) | None => widget::container(retro::dim(p, "…"))
                .padding(8)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into(),
        };
        pic
    }

    /// One inline image: frame treatment, ⋯ menu, drag grip, and the popup menu.
    fn image_card<'a>(
        &'a self,
        p: &Palette,
        r: &'a ImageRef,
        block: usize,
    ) -> Element<'a, Message> {
        let pic = self.image_picture(p, r, block);

        // Click the picture for the menu, drag it to move it; drag ◢ to resize.
        let clickable = widget::mouse_area(pic)
            .on_press(Message::ImagePress(block))
            .interaction(mouse::Interaction::Grab);
        let menu_open = self.image_menu == Some(block);
        let dots = widget::container(
            widget::button::custom(retro::accent(p, "⋯").size(14))
                .padding([0, 6])
                .class(retro::row_class(p, menu_open))
                .on_press(Message::ImageMenu(Some(block))),
        )
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding([14, 6, 0, 0]);
        let grip = widget::container(
            widget::mouse_area(widget::container(retro::accent(p, "◢").size(13)).padding([0, 3]))
                .on_press(Message::ResizeStart(block))
                .interaction(mouse::Interaction::ResizingHorizontally),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(2);
        let stacked = cosmic::iced::widget::stack([clickable.into(), dots.into(), grip.into()])
            .width(Length::Fill);

        let sized = self.size_card(r, block, stacked.into());

        if menu_open {
            widget::popover(sized)
                .popup(self.image_menu_view(p, r, block))
                .position(widget::popover::Position::Bottom)
                .on_close(Message::ImageMenu(None))
                .into()
        } else {
            sized.into()
        }
    }

    /// 1-based position of this image among the note's images (film frame number).
    fn image_number(&self, block: usize) -> usize {
        self.blocks
            .images()
            .iter()
            .position(|(b, _)| *b == block)
            .map_or(1, |i| i + 1)
    }

    /// The ⋯ menu: frame styles, alignment, width, open, remove.
    fn image_menu_view<'a>(
        &'a self,
        p: &Palette,
        r: &'a ImageRef,
        block: usize,
    ) -> Element<'a, Message> {
        let item = |label: String, selected: bool, msg: Message| {
            widget::button::custom(
                widget::row::with_capacity(2)
                    .push(retro::accent(p, if selected { "▌" } else { " " }))
                    .push(retro::text(p, label))
                    .spacing(6),
            )
            .padding([2, 8])
            .width(Length::Fill)
            .class(retro::row_class(p, selected))
            .on_press(msg)
        };
        let mut col = widget::column::with_capacity(22)
            .spacing(1)
            .width(Length::Fixed(220.0));
        col = col.push(widget::container(retro::dim(p, fl!("menu-caption"))).padding([2, 8]));
        col = col.push(
            widget::container(
                widget::text_input(fl!("caption-placeholder"), &r.alt)
                    .font(retro::mono())
                    .size(13)
                    .padding([3, 8])
                    .style(retro::search_class(p))
                    .on_input(move |t| Message::SetCaption(block, t))
                    .on_submit(|_| Message::ImageMenu(None)),
            )
            .padding([0, 6, 4, 6]),
        );
        col = col.push(widget::container(retro::dim(p, fl!("menu-frame"))).padding([2, 8]));
        for style in FrameStyle::ALL {
            col = col.push(item(
                style.label().to_owned(),
                r.frame == style,
                Message::SetFrame(block, style),
            ));
        }
        col = col.push(widget::container(retro::dim(p, fl!("menu-align"))).padding([6, 8, 2, 8]));
        let mut align_row = widget::row::with_capacity(3).spacing(2);
        for a in Align::ALL {
            align_row = align_row.push(
                widget::button::custom(retro::text(p, a.label()))
                    .padding([2, 8])
                    .class(retro::row_class(p, r.align == a))
                    .on_press(Message::SetAlign(block, a)),
            );
        }
        col = col.push(widget::container(align_row).padding([0, 6]));
        col = col.push(widget::container(retro::dim(p, fl!("menu-width"))).padding([6, 8, 2, 8]));
        let mut width_row = widget::row::with_capacity(4).spacing(2);
        for (label, px) in images::WIDTH_PRESETS {
            width_row = width_row.push(
                widget::button::custom(retro::text(p, label))
                    .padding([2, 6])
                    .class(retro::row_class(p, r.width == Some(px)))
                    .on_press(Message::SetWidth(block, Some(px))),
            );
        }
        width_row = width_row.push(
            widget::button::custom(retro::text(p, fl!("menu-full")))
                .padding([2, 6])
                .class(retro::row_class(p, r.width.is_none()))
                .on_press(Message::SetWidth(block, None)),
        );
        col = col.push(widget::container(width_row).padding([0, 6]));
        col = col.push(
            widget::container(retro::dim(p, fl!("menu-resize-hint")).size(11)).padding([2, 8]),
        );
        col = col.push(
            widget::container(retro::dim(p, fl!("menu-move-hint")).size(11)).padding([0, 8, 2, 8]),
        );
        col = col.push(
            widget::row::with_capacity(2)
                .push(item(
                    fl!("open-image"),
                    false,
                    Message::OpenImage(r.path.clone()),
                ))
                .push(item(fl!("menu-remove"), false, Message::RemoveImage(block)))
                .spacing(2),
        );
        widget::container(col)
            .padding([6, 4])
            .class(retro::dock_class(p))
            .into()
    }

    fn persist_layout(&mut self) {
        if let Some(handler) = &self.config_handler {
            if let Err(why) = self.config.set_hide_nav(handler, !self.show_nav) {
                tracing::warn!(%why, "could not persist nav visibility");
            }
            if let Err(why) = self.config.set_hide_list(handler, !self.show_list) {
                tracing::warn!(%why, "could not persist list visibility");
            }
        }
    }

    // ----- state -----

    /// Reload the tag tree and view counts from the store.
    /// A local change: sync once typing settles.
    fn sync_soon(&mut self) {
        if self.sync.session.is_some() {
            let at = Instant::now() + SYNC_AFTER_SAVE;
            self.sync.due = Some(self.sync.due.map_or(at, |d| d.min(at)));
        }
    }

    /// Run one cycle in the background, unless one is already going.
    fn start_sync(&mut self) -> Task<cosmic::Action<Message>> {
        if self.sync.busy {
            return Task::none();
        }
        let (Some(store), Some(session)) = (self.store.as_ref(), self.sync.session.clone()) else {
            return Task::none();
        };
        let job = match store.sync_job(session, self.config.device_id.clone()) {
            Ok(job) => job,
            Err(err) => {
                tracing::error!(%err, "gathering sync work");
                self.sync.error = Some(format!("{err:#}"));
                self.sync.last_run = Some(Instant::now());
                self.sync.due = None;
                return Task::none();
            }
        };
        self.sync.busy = true;
        self.sync.due = None;
        Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || sync::run(job)).await {
                    Ok(out) => out,
                    Err(err) => sync::Outcome {
                        errors: vec![err.to_string()],
                        ..Default::default()
                    },
                }
            },
            |out| cosmic::Action::App(Message::SyncDone(Arc::new(out))),
        )
    }

    fn refresh_tags(&mut self) {
        let Some(store) = &self.store else { return };
        let tags = store.tags().unwrap_or_default();
        self.tags = tag_tree(&tags);
        self.view_counts = [
            store.list(&View::All).map_or(0, |v| v.len()),
            store.list(&View::Untagged).map_or(0, |v| v.len()),
            store.list(&View::Trash).map_or(0, |v| v.len()),
        ];
        // The tag we were looking at may be gone.
        if let View::Tag(t) = &self.view
            && !self.tags.iter().any(|(n, _)| n == t)
        {
            self.view = View::All;
        }
    }

    fn refresh_list(&mut self) {
        let Some(store) = &self.store else { return };
        self.notes = match store.search(&self.query, &self.view) {
            Ok(notes) => notes,
            Err(err) => {
                tracing::error!(%err, "listing notes");
                Vec::new()
            }
        };
    }

    /// Tags or trash membership may have changed: rebuild nav + list.
    fn after_store_change(&mut self) {
        self.sync_soon();
        self.refresh_tags();
        self.refresh_list();
        if self.current.is_none()
            && let Some(first) = self.notes.first().map(|n| n.id.clone())
        {
            self.open_note(&first);
        }
    }

    /// Make a note with `body`, open it and focus the editor; the caret
    /// lands after the title line, or on a fresh paragraph below it when
    /// `below_title`.
    fn create_note(&mut self, body: String, below_title: bool) -> Task<cosmic::Action<Message>> {
        self.close_current();
        let created = self.store.as_mut().and_then(|s| match s.create() {
            Ok(mut note) => {
                note.body = body;
                if let Err(err) = s.save(&mut note) {
                    tracing::error!(%err, "pre-filling new note");
                }
                Some(note)
            }
            Err(err) => {
                tracing::error!(%err, "creating note");
                None
            }
        });
        if let Some(note) = created {
            self.refresh_tags();
            self.refresh_list();
            self.open_note(&note.id);
            if let Some(c) = self.blocks.focused_text() {
                c.perform(text_editor::Action::Move(text_editor::Motion::End));
                if below_title {
                    c.perform(text_editor::Action::Edit(text_editor::Edit::Enter));
                    c.perform(text_editor::Action::Edit(text_editor::Edit::Enter));
                }
            }
            return Task::batch([self.update_title(), self.focus_editor()]);
        }
        self.update_title()
    }

    fn open_note(&mut self, id: &str) {
        let Some(store) = self.store.as_mut() else {
            return;
        };
        match store.load(id) {
            Ok(Some(note)) => {
                self.blocks = Blocks::from_body(&note.body);
                // Byte 0 sits before the title's hidden `# `; a keystroke
                // there breaks the title. Start after the title instead.
                if let Some(content) = self.blocks.focused_text() {
                    content.perform(text_editor::Action::Move(text_editor::Motion::End));
                    content.request_reveal();
                }
                self.image_menu = None;
                self.undo.clear();
                self.redo.clear();
                self.last_undo_kind = EditKind::Other;
                self.backlinks = store.backlinks(&note.title).unwrap_or_default();
                self.current = Some(note);
                self.dirty = false;
            }
            Ok(None) => {
                self.current = None;
                self.refresh_list();
            }
            Err(err) => tracing::error!(%err, id, "loading note"),
        }
    }

    /// Write pending edits and let go of the current note.
    fn close_current(&mut self) {
        // An open cell's draft belongs to the note being written out.
        self.commit_table_edit();
        self.table_pick = None;
        self.table_fill = None;
        self.table_sel = None;
        self.flush();
        self.dragging = None;
        self.resizing = None;
        self.live_width = None;
        let Some(note) = self.current.take() else {
            return;
        };
        self.backlinks.clear();
        if let Some(store) = self.store.as_mut() {
            match store.delete_if_empty(&note.id) {
                Ok(true) => {
                    self.refresh_tags();
                    self.refresh_list();
                }
                Ok(false) => {}
                Err(err) => tracing::error!(%err, "dropping empty note"),
            }
        }
    }

    /// Persist the editor buffer if it has unsaved changes.
    fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (Some(store), Some(note)) = (self.store.as_mut(), self.current.as_mut()) else {
            return;
        };
        let old_title = note.title.clone();
        note.body = self.blocks.body();
        if let Err(err) = store.save(note) {
            tracing::error!(%err, "saving note");
            self.save_error = Some(format!("{err:#}"));
            // Keep the edit pending and retry in a few seconds.
            self.dirty = true;
            self.last_edit = Instant::now() + Duration::from_secs(5);
            return;
        }
        self.save_error = None;
        if note.title != old_title {
            self.backlinks = store.backlinks(&note.title).unwrap_or_default();
        }
        self.sync_soon();
        // Tags may have changed; keep nav + list in step.
        self.refresh_tags();
        self.refresh_list();
    }

    /// Apply one `JJB_SCRIPT` step by routing it through the normal messages.
    fn run_step(&mut self, step: Step) -> Task<cosmic::Action<Message>> {
        match step {
            Step::New => self.update(Message::NewNote),
            Step::Type(text) => {
                // Type one line per tick so the editor sees the same incremental
                // edits a keyboard would produce.
                let (now, rest) = match text.find('\n') {
                    Some(i) => (text[..=i].to_owned(), text[i + 1..].to_owned()),
                    None => (text, String::new()),
                };
                if !rest.is_empty()
                    && let Some(runner) = self.script.as_mut()
                {
                    runner.push_front(Step::Type(rest));
                }
                let text = now;
                let mut task = Task::none();
                for c in text.chars() {
                    let edit = if c == '\n' {
                        text_editor::Edit::Enter
                    } else {
                        text_editor::Edit::Insert(c)
                    };
                    task = self.update(Message::Editor(
                        self.blocks.focused,
                        text_editor::Action::Edit(edit),
                    ));
                }
                task
            }
            Step::Search(text) => self.update(Message::Search(text)),
            Step::Select(n) => match self.notes.get(n).map(|s| s.id.clone()) {
                Some(id) => self.update(Message::Select(id)),
                None => Task::none(),
            },
            Step::Pin => self.update(Message::TogglePin),
            Step::Folder(name) => {
                self.new_folder = name;
                self.update(Message::CreateFolder)
            }
            Step::Format(key) => match Format::ALL.into_iter().find(|f| f.key() == key) {
                Some(format) => self.update(Message::Format(format)),
                None => Task::none(),
            },
            Step::SelectAll => self.update(Message::Editor(
                self.blocks.focused,
                text_editor::Action::SelectAll,
            )),
            Step::Dock => self.update(Message::ToggleDock),
            Step::Undo => self.update(Message::Undo),
            Step::Shortcuts => self.update(Message::ToggleShortcuts),
            Step::SavedInfo => self.update(Message::ToggleSavedInfo),
            Step::Redo => self.update(Message::Redo),
            Step::Themes => self.update(Message::ToggleContextPage(ContextPage::Themes)),
            Step::Solo => self.update(Message::ToggleSolo),
            Step::Image(path) => self.update(Message::ImagesDropped(vec![PathBuf::from(path)])),
            Step::Pick => self.update(Message::PickImage),
            Step::Attach(path) => self.import_file_at(std::path::Path::new(&path), None),
            Step::PickDir(dir) => self.update(Message::PickerNavigate(PathBuf::from(dir))),
            Step::ImgFrame(n, key) => match (self.nth_image_block(n), FrameStyle::from_key(&key)) {
                (Some(b), Some(f)) => self.update(Message::SetFrame(b, f)),
                _ => Task::none(),
            },
            Step::ImgAlign(n, key) => match (self.nth_image_block(n), Align::from_key(&key)) {
                (Some(b), Some(a)) => self.update(Message::SetAlign(b, a)),
                _ => Task::none(),
            },
            Step::ImgWidth(n, w) => match self.nth_image_block(n) {
                Some(b) => self.update(Message::SetWidth(b, (w > 0).then_some(w))),
                None => Task::none(),
            },
            Step::ImgCaption(n, text) => match self.nth_image_block(n) {
                Some(b) => self.update(Message::SetCaption(b, text)),
                None => Task::none(),
            },
            Step::ImgMenu(n) => match self.nth_image_block(n) {
                Some(b) => self.update(Message::ImageMenu(Some(b))),
                None => Task::none(),
            },
            Step::ImgDrag(n, line) | Step::ImgMove(n, line) => match self.nth_image_block(n) {
                Some(b) => {
                    self.dragging = Some(ImageDrag {
                        block: b,
                        start: Point::ORIGIN,
                        active: true,
                        target: None,
                    });
                    let over = self.update(Message::DragOver(line));
                    if matches!(step, Step::ImgMove(..)) {
                        drop(over);
                        self.update(Message::MouseReleased)
                    } else {
                        over
                    }
                }
                None => Task::none(),
            },
            Step::LinkDrag(n, line) => match self.blocks.links().get(n).map(|(b, _)| *b) {
                Some(b) => {
                    self.dragging = Some(ImageDrag {
                        block: b,
                        start: Point::ORIGIN,
                        active: true,
                        target: None,
                    });
                    self.update(Message::DragOver(line))
                }
                None => Task::none(),
            },
            Step::Theme(key) => self.update(Message::SetTheme(retro::Theme::from_key(&key))),
            Step::Pick2(row, col, over) => {
                let table = self
                    .blocks
                    .items
                    .iter()
                    .position(|b| matches!(b, Block::Table(_)));
                if let Some(block) = table {
                    let _ = self.update(Message::TablePick(block, row, col));
                    if let Some((r2, c2)) = over {
                        return self.update(Message::TablePickOver(block, r2, c2));
                    }
                }
                Task::none()
            }
            Step::PickOver2(row, col) => {
                match self.table_pick.as_ref().map(|p| p.block) {
                    Some(block) => self.update(Message::TablePickOver(block, row, col)),
                    None => Task::none(),
                }
            }
            Step::PickDone => self.update(Message::MouseReleased),
            Step::Draft(text) => self.update(Message::TableDraft(text)),
            Step::TSel(r, c, r2, c2) => {
                let table = self
                    .blocks
                    .items
                    .iter()
                    .position(|b| matches!(b, Block::Table(_)));
                if let Some(block) = table {
                    let _ = self.update(Message::TableSelStart(block, r, c));
                    return self.update(Message::TableSelOver(block, r2, c2));
                }
                Task::none()
            }
            Step::Fill(r, c, r2, c2) => {
                let table = self
                    .blocks
                    .items
                    .iter()
                    .position(|b| matches!(b, Block::Table(_)));
                if let Some(block) = table {
                    let _ = self.update(Message::TableFillStart(block, r, c));
                    let _ = self.update(Message::TableFillOver(block, r2, c2));
                    return self.update(Message::MouseReleased);
                }
                Task::none()
            }
            Step::EditCell(row, col) => {
                let table = self
                    .blocks
                    .items
                    .iter()
                    .position(|b| matches!(b, Block::Table(_)));
                match table {
                    Some(block) => self.update(Message::TableEdit(block, row, col)),
                    None => Task::none(),
                }
            }
            Step::Cell(row, col, text) => {
                let table = self
                    .blocks
                    .items
                    .iter()
                    .position(|b| matches!(b, Block::Table(_)));
                if let Some(block) = table {
                    if let Some(t) = self.blocks.table_mut(block) {
                        while t.rows.len() <= row {
                            t.add_row();
                        }
                        while t.cols() <= col {
                            t.add_col();
                        }
                    }
                    let _ = self.update(Message::TableEdit(block, row, col));
                    let _ = self.update(Message::TableDraft(text));
                    return self.update(Message::TableCommit(TableMove::Stay));
                }
                Task::none()
            }
            Step::Sel(line, col, to) => {
                if let Some(editor) = self.blocks.focused_text() {
                    // The anchor is the selection field; the caret the position.
                    let (position, selection) = match to {
                        Some((l2, c2)) => (
                            text_editor::Position {
                                line: l2,
                                column: c2,
                            },
                            Some(text_editor::Position { line, column: col }),
                        ),
                        None => (text_editor::Position { line, column: col }, None),
                    };
                    editor.move_to(text_editor::Cursor {
                        position,
                        selection,
                    });
                }
                Task::none()
            }
            Step::Sweep(line, col, l2, c2) => {
                let _ = self.run_step(Step::Sel(line, col, Some((l2, c2))));
                self.update(Message::MarkSweep)
            }
            Step::Buffet(highlight, dark) => {
                let _ = self.update(Message::SetBuffetHighlight(retro::Theme::from_key(
                    &highlight,
                )));
                self.update(Message::SetBuffetDark(retro::Dark::from_key(&dark)))
            }
            Step::BuffetLight(highlight, paper, ink) => {
                let _ = self.update(Message::SetBuffetHighlight(retro::Theme::from_key(
                    &highlight,
                )));
                let _ = self.update(Message::SetBuffetLight(retro::Light::from_key(&paper)));
                self.update(Message::SetBuffetInk(retro::Ink::from_key(&ink)))
            }
            Step::Fold(tag) => self.update(Message::ToggleTagFold(tag)),
            Step::Nav(delta) => self.update(Message::NavigateNotes(delta)),
            Step::ToggleBox(line, col) => {
                let block = self.blocks.focused;
                if let Some(c) = self.blocks.text_mut(block) {
                    c.move_to(text_editor::Cursor {
                        position: text_editor::Position { line, column: col },
                        selection: None,
                    });
                }
                let before = self.snapshot();
                let marker = self.task_marker.clone();
                if let Some(c) = self.blocks.text_mut(block)
                    && crate::blocks::toggle_task_at_cursor(c, &marker)
                {
                    self.push_undo(before);
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
                Task::none()
            }
            Step::TagMenu(tag) => self.update(Message::TagMenu(Some(tag))),
            Step::TagDrag(entry, slot) | Step::TagMove(entry, slot) => {
                self.tag_drag = Some(TagDrag {
                    entry,
                    start: Point::ORIGIN,
                    active: true,
                    target: None,
                });
                let over = self.update(Message::TagDragOver(slot));
                if matches!(step, Step::TagMove(..)) {
                    drop(over);
                    self.update(Message::MouseReleased)
                } else {
                    over
                }
            }
            Step::AddSpace => self.update(Message::AddSpace),
            Step::Weight(w) => self.update(Message::SetWeight(w)),
            Step::FontFor(pane) => self.update(Message::SetFontTarget(match pane.as_str() {
                "tags" => Pane::Sidebar,
                "notes" => Pane::List,
                _ => Pane::Editor,
            })),
            Step::Quit => self.update(Message::Quit),
            Step::Sync(url, email, password, new) => {
                self.sync.url = url;
                self.sync.email = email;
                self.sync.password = password;
                self.update(if new { Message::SyncSignUp } else { Message::SyncSignIn })
            }
            Step::SyncNow => self.update(Message::SyncNow),
            Step::RenameTag(old, new) => {
                self.tag_rename = Some((old, new));
                self.update(Message::TagRenameCommit)
            }
            Step::Font(key) => self.update(Message::SetFont(retro::EditorFont::from_key(&key))),
            Step::Pairing(key) => self.update(Message::SetPairing(key)),
            Step::Section(name) => self.update(Message::ToggleSection(match name.as_str() {
                "colour" | "color" => Section::Colour,
                "font" => Section::Font,
                "tasks" => Section::Tasks,
                "icon" => Section::Icon,
                "links" => Section::Links,
                "buffet" => Section::Buffet,
                "tables" => Section::Tables,
                "sync" => Section::Sync,
                _ => Section::Size,
            })),
            Step::Marker(mark) => self.update(Message::SetTaskMarker(mark)),
            Step::Measure(key) => self.update(Message::SetMeasure(retro::Measure::from_key(&key))),
            Step::Coffee => self.update(Message::ToggleCoffee),
            Step::IconSet(key) => {
                self.update(Message::SetIconSet(crate::glyph::IconSet::from_key(&key)))
            }
            Step::TagIcon(tag, key) => {
                self.update(Message::SetTagIcon(tag, crate::glyph::Icon::from_key(&key)))
            }
            Step::Icon(key) => self.update(Message::SetIcon(
                (key != "follow").then(|| retro::Theme::from_key(&key)),
            )),
            Step::Follow(title) => self.update(Message::FollowLink(
                crate::editor::widget::Link::Note(title),
            )),
            Step::Size(pane, delta) => {
                let pane = match pane.as_str() {
                    "sidebar" => Pane::Sidebar,
                    "list" => Pane::List,
                    _ => Pane::Editor,
                };
                self.update(Message::SizeStep(pane, delta))
            }
            Step::DockSize(key) => {
                self.update(Message::SetDockSize(retro::DockSize::from_key(&key)))
            }
            Step::Trash => self.update(Message::TrashCurrent),
            Step::Wait(_) => Task::none(),
            Step::Key(name, times) => {
                use text_editor::{Action, Edit, Motion};
                let action = match name.as_str() {
                    "backspace" => Action::Edit(Edit::Backspace),
                    "delete" => Action::Edit(Edit::Delete),
                    "enter" => Action::Edit(Edit::Enter),
                    "tab" => Action::Edit(Edit::Indent),
                    "left" => Action::Move(Motion::Left),
                    "right" => Action::Move(Motion::Right),
                    "up" => Action::Move(Motion::Up),
                    "down" => Action::Move(Motion::Down),
                    "home" => Action::Move(Motion::Home),
                    "end" => Action::Move(Motion::End),
                    _ => return Task::none(),
                };
                let mut task = Task::none();
                for _ in 0..times {
                    task = self.update(Message::Editor(self.blocks.focused, action.clone()));
                }
                task
            }
            Step::Exit => {
                self.close_current();
                std::process::exit(0);
            }
        }
    }

    fn nth_image_block(&self, n: usize) -> Option<usize> {
        self.blocks.images().get(n).map(|(b, _)| *b)
    }

    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");
        if let Some(note) = &self.current {
            window_title.push_str(" — ");
            window_title.push_str(&note.title);
        }
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }
}

fn header_button<'a>(
    icon_name: &'static str,
    label: String,
    on_press: Message,
) -> Element<'a, Message> {
    widget::tooltip(
        widget::button::icon(icon::from_name(icon_name)).on_press(on_press),
        widget::text::body(label),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn note_row<'a>(
    p: &Palette,
    note: &'a NoteSummary,
    selected: bool,
    size: u16,
    ui: cosmic::font::Font,
) -> Element<'a, Message> {
    let fg = if selected { p.selfg } else { p.fg };
    let size = f32::from(size);
    let mut title_row = widget::row::with_capacity(3)
        .spacing(8)
        .align_y(Alignment::Center);
    if note.pinned {
        title_row = title_row.push(retro::accent(p, "▲").size(size));
    }
    title_row = title_row
        .push(
            retro::text(p, note.title.clone())
                .font(cosmic::font::Font {
                    weight: cosmic::iced::font::Weight::Bold,
                    ..ui
                })
                .size(size)
                .class(cosmic::theme::Text::Color(fg))
                .width(Length::Fill),
        )
        .push(
            retro::dim(p, format_date(note.modified))
                .font(ui)
                .size(size - 2.0),
        );

    let mut column = widget::column::with_capacity(2)
        .push(title_row)
        .spacing(3)
        .width(Length::Fill);
    if !note.preview.is_empty() {
        let preview = note::clip_words(&note.preview, 90);
        column = column.push(retro::dim(p, preview).font(ui).size(size - 1.0));
    }
    column.into()
}

/// Relative dates: time today, day+month this year, full date otherwise.
fn format_date(when: DateTime<Utc>) -> String {
    let local = when.with_timezone(&Local);
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%-d %b").to_string()
    } else {
        local.format("%-d %b %Y").to_string()
    }
}

/// "just now", "2 min ago", "3 h ago".
fn ago(d: Duration) -> String {
    let secs: u64 = d.as_secs();
    let (mins, hours): (u64, u64) = (secs / 60, secs / 3600);
    if secs < 10 {
        fl!("ago-now")
    } else if secs < 60 {
        fl!("ago-seconds", n = secs)
    } else if secs < 3600 {
        fl!("ago-minutes", n = mins)
    } else {
        fl!("ago-hours", n = hours)
    }
}

fn format_time(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local).format("%H:%M").to_string()
}

/// Expand `a/b/c` tags into a depth-first tree, inserting implicit parents
/// (`a`, `a/b`) that no note carries directly with a count of 0.
/// Register every bundled font with the renderer (the title font and the
/// editor faces). Safe to repeat: already-loaded byte slices are skipped.
fn load_fonts() -> Task<cosmic::Action<Message>> {
    let load = |bytes: &'static [u8]| {
        cosmic::iced::font::load(bytes).map(|result| {
            if let Err(err) = result {
                tracing::error!(?err, "loading bundled font");
            }
            cosmic::Action::App(Message::FontLoaded)
        })
    };
    let mut tasks = vec![load(retro::TITLE_FONT_BYTES)];
    tasks.extend(retro::EDITOR_FONT_FILES.iter().map(|b| load(b)));
    Task::batch(tasks)
}

/// Config stores 0 for "default"; anything else is clamped to the allowed range.
fn size_from_config(px: u16, default: u16, min: u16, max: u16) -> u16 {
    if px == 0 { default } else { px.clamp(min, max) }
}

fn pane_size_from_config(px: u16) -> u16 {
    size_from_config(
        px,
        retro::PANE_SIZE_DEFAULT,
        retro::PANE_SIZE_MIN,
        retro::PANE_SIZE_MAX,
    )
}

/// A text column whose size the user can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    List,
    Editor,
}

/// Cloud sync, as the app sees it.
#[derive(Debug, Default)]
struct SyncUi {
    url: String,
    email: String,
    password: String,
    show_password: bool,
    /// Signed in. The token is kept in the keyring, not config.
    session: Option<Session>,
    /// A cycle (or a sign-in) is in flight.
    busy: bool,
    /// Still asking the keyring for a remembered token.
    restoring: bool,
    /// Run a cycle at this time (a local change settling).
    due: Option<Instant>,
    last_run: Option<Instant>,
    last_ok: Option<Instant>,
    error: Option<String>,
    /// What the last cycle did, for the status line.
    summary: Option<String>,
    /// The same for pictures and attached files.
    files_summary: Option<String>,
}

/// Foldable sections of the Appearance drawer (index into `appearance_open`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Colour = 0,
    Font = 1,
    Size = 2,
    Tasks = 3,
    Icon = 4,
    Links = 5,
    Buffet = 6,
    Tables = 7,
    Sync = 8,
}

/// Is `line[ws..we]` wrapped in `format`'s markers? Star formats count the
/// run of `*` on each side, since `*`, `**` and `***` overlap: italic is an
/// odd run, bold a run of two or more. The rest compare markers directly.
fn wrapped_in(line: &str, ws: usize, we: usize, format: Format, before: &str, after: &str) -> bool {
    let star_runs = || {
        let prefix = line[..ws].chars().rev().take_while(|c| *c == '*').count();
        let suffix = line[we..].chars().take_while(|c| *c == '*').count();
        (prefix, suffix)
    };
    match format {
        Format::Bold => {
            let (p, s) = star_runs();
            p >= 2 && s >= 2
        }
        Format::Italic => {
            let (p, s) = star_runs();
            p % 2 == 1 && s % 2 == 1
        }
        _ => line[..ws].ends_with(before) && line[we..].starts_with(after),
    }
}

/// The cells a fill drag covers: clamped to one axis, whichever the sweep
/// moved along furthest. `None` when it never left the source cell.
fn fill_targets(
    f: TableFill,
) -> Option<(
    std::ops::RangeInclusive<usize>,
    std::ops::RangeInclusive<usize>,
)> {
    let dr = f.current.0 as isize - f.from.0 as isize;
    let dc = f.current.1 as isize - f.from.1 as isize;
    if dr == 0 && dc == 0 {
        return None;
    }
    if dr.abs() >= dc.abs() {
        let (a, b) = (f.from.0.min(f.current.0), f.from.0.max(f.current.0));
        Some((a..=b, f.from.1..=f.from.1))
    } else {
        let (a, b) = (f.from.1.min(f.current.1), f.from.1.max(f.current.1));
        Some((f.from.0..=f.from.0, a..=b))
    }
}

/// `(row, col)` → the formula's name for it: `B2`.
fn cell_ref(row: usize, col: usize) -> String {
    format!("{}{}", crate::table::col_name(col), row + 1)
}

/// Does the formula draft want a reference next? True after `=`, an
/// operator, an opening paren, a separator, or a range colon — the same
/// moments Excel lets you point at cells.
fn ref_expected(draft: &str) -> bool {
    draft.starts_with('=')
        && draft
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|c| matches!(c, '=' | '(' | '+' | '-' | '*' | '/' | ',' | ';' | ':'))
}

/// Byte bounds of the word touching byte `col` in `line` (alphanumerics
/// plus `_` and `'`; the caret may sit inside or at either edge).
fn word_bounds(line: &str, col: usize) -> Option<(usize, usize)> {
    let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '\'');
    let mut start = 0;
    let mut in_word = false;
    for (i, c) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ' ')))
    {
        if is_word(c) {
            if !in_word {
                start = i;
            }
            in_word = true;
        } else {
            if in_word && start <= col && col <= i {
                return Some((start, i));
            }
            in_word = false;
        }
    }
    None
}

/// The buffet highlight from config: a minimal theme key, or Tomato.
fn buffet_highlight_from_config(s: &str) -> retro::Theme {
    let t = retro::Theme::from_key(s);
    if t.is_minimal() { t } else { retro::Theme::Tomato }
}

fn title_font_from_config(s: &str) -> retro::EditorFont {
    if s.is_empty() {
        retro::EditorFont::Vt323
    } else {
        retro::EditorFont::from_key(s)
    }
}

fn task_marker_from_config(s: &str) -> String {
    if s.is_empty() {
        "x".to_owned()
    } else {
        s.to_owned()
    }
}

/// Where a line's own text starts: past a heading's hashes, a quote or
/// list marker, a task box, or a typed `•` bullet. Highlights begin here.
fn lead_in(line: &str) -> usize {
    let t = line.trim_start();
    let indent = line.len() - t.len();
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        return indent + hashes + 1;
    }
    if let Some((len, _)) = line_prefix(line) {
        return len;
    }
    match t.chars().next() {
        Some(c) if matches!(c, '•' | '◦' | '▪' | '‣' | '–') && t[c.len_utf8()..].starts_with(' ') => {
            indent + c.len_utf8() + 1
        }
        _ => 0,
    }
}

/// The highlighter: mark the selection (each line gets its own `==…==`,
/// since a mark cannot span lines), or rub the highlight out when all of
/// it is already marked. A lone caret inside a mark rubs that mark out.
/// `false` when there was nothing for it to do, so the caller's plain
/// caret handling (wrap the word, plant a pair) takes over.
fn mark_selection(editor: &mut crate::editor::Content, sweep: bool) -> bool {
    use text_editor::{Action, Cursor, Edit, Position};
    let cursor = editor.cursor();
    let pos = cursor.position;
    let text_of = |editor: &crate::editor::Content, l: usize| {
        editor.line(l).map(|t| t.text.into_owned()).unwrap_or_default()
    };
    let replace_line = |editor: &mut crate::editor::Content, l: usize, old: &str, new: String| {
        editor.move_to(Cursor {
            position: Position { line: l, column: old.len() },
            selection: Some(Position { line: l, column: 0 }),
        });
        editor.perform(Action::Edit(Edit::Paste(Arc::new(new))));
    };
    let Some(anchor) = cursor.selection else {
        let text = text_of(editor, pos.line);
        let Some(m) = markdown::mark_around(&text, pos.column, pos.column) else {
            return false;
        };
        let (new, freed) = markdown::unmark(&text, m);
        replace_line(editor, pos.line, &text, new);
        editor.move_to(Cursor {
            position: Position {
                line: pos.line,
                column: pos.column.saturating_sub(2).clamp(freed.start, freed.end),
            },
            selection: None,
        });
        return true;
    };
    let (from, to) = if (anchor.line, anchor.column) <= (pos.line, pos.column) {
        (anchor, pos)
    } else {
        (pos, anchor)
    };
    // What to mark on each line the selection touches.
    let mut parts = Vec::new();
    for l in from.line..=to.line {
        let text = text_of(editor, l);
        let mut a = if l == from.line { from.column.min(text.len()) } else { 0 };
        let mut b = if l == to.line { to.column.min(text.len()) } else { text.len() };
        if sweep && a < b {
            let snapped = markdown::snap_to_words(&text, a, b);
            if l == from.line {
                a = snapped.start;
            }
            if l == to.line {
                b = snapped.end;
            }
        }
        a = a.max(lead_in(&text));
        if let Some(r) = markdown::trim_range(&text, a, b) {
            parts.push((l, text, r));
        }
    }
    let rub_out = parts
        .iter()
        .all(|(_, text, r)| markdown::mark_around(text, r.start, r.end).is_some());
    let mut last = None;
    for (l, text, r) in parts {
        let done = if rub_out {
            markdown::mark_around(&text, r.start, r.end).map(|m| markdown::unmark(&text, m))
        } else {
            markdown::mark(&text, r.start, r.end)
        };
        if let Some((new, at)) = done {
            replace_line(editor, l, &text, new);
            last = Some((l, at));
        }
    }
    let one_line = from.line == to.line;
    match last {
        // From the keyboard the words stay selected, so the same keys
        // toggle straight back; the pen just moves on.
        Some((l, at)) if one_line && !sweep => editor.move_to(Cursor {
            position: Position { line: l, column: at.end },
            selection: Some(Position { line: l, column: at.start }),
        }),
        // Past the closing marker: what is typed next is not highlighted.
        Some((l, at)) => editor.move_to(Cursor {
            position: Position {
                line: l,
                column: if rub_out { at.end } else { at.end + 2 },
            },
            selection: None,
        }),
        None => editor.perform(Action::ClearSelection),
    }
    true
}

/// The block prefix a line already carries and the dock action that makes
/// it: `# ` (H1), `## ` (H2), `- ` (bullet), `- [ ] ` / `- [✓] ` (to-do).
fn line_prefix(line: &str) -> Option<(usize, Format)> {
    if line.starts_with("## ") {
        return Some((3, Format::H2));
    }
    if line.starts_with("# ") {
        return Some((2, Format::H1));
    }
    if line.starts_with("> ") {
        return Some((2, Format::Quote));
    }
    let lm = note::list_marker(line)?;
    match note::task_box(&line[lm..]) {
        Some((len, _)) => Some((lm + len, Format::Todo)),
        None => Some((lm, Format::Bullet)),
    }
}

fn tag_tree(tags: &[(String, usize)]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for (name, count) in tags {
        let mut prefix = String::new();
        let segments: Vec<&str> = name.split('/').collect();
        for (i, segment) in segments.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            let is_leaf = i + 1 == segments.len();
            match out.iter_mut().find(|(n, _)| *n == prefix) {
                Some(existing) if is_leaf => existing.1 = *count,
                Some(_) => {}
                None => out.push((prefix.clone(), if is_leaf { *count } else { 0 })),
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn key_binds() -> HashMap<menu::KeyBind, MenuAction> {
    use keyboard::Key;
    use menu::key_bind::Modifier::{Alt, Ctrl, Shift};
    let mut binds = HashMap::new();
    let mut bind = |modifiers: &[menu::key_bind::Modifier], key: &str, action: MenuAction| {
        binds.insert(
            menu::KeyBind {
                modifiers: modifiers.to_vec(),
                key: Key::Character(key.into()),
            },
            action,
        );
    };
    // File
    bind(&[Ctrl], "n", MenuAction::NewNote);
    bind(&[Ctrl, Shift], "n", MenuAction::NewFolder);
    bind(&[Ctrl, Shift], "i", MenuAction::AddImage);
    bind(&[Ctrl, Shift], "a", MenuAction::AttachFile);
    bind(&[Ctrl], "f", MenuAction::FocusSearch);
    bind(&[Ctrl, Shift], "p", MenuAction::Pin);
    bind(&[Ctrl, Shift], "d", MenuAction::TrashNote);
    bind(&[Ctrl], "q", MenuAction::Quit);
    // Edit
    bind(&[Ctrl], "z", MenuAction::Undo);
    bind(&[Ctrl, Shift], "z", MenuAction::Redo);
    bind(&[Ctrl], "y", MenuAction::Redo);
    // Format
    bind(&[Ctrl], "b", MenuAction::Format(Format::Bold));
    bind(&[Ctrl], "i", MenuAction::Format(Format::Italic));
    bind(&[Ctrl], "e", MenuAction::Format(Format::Code));
    bind(&[Ctrl], "h", MenuAction::Format(Format::Mark));
    bind(&[Ctrl], "1", MenuAction::Format(Format::H1));
    bind(&[Ctrl], "2", MenuAction::Format(Format::H2));
    bind(&[Ctrl], "l", MenuAction::Format(Format::Bullet));
    bind(&[Ctrl], "t", MenuAction::Format(Format::Todo));
    bind(&[Ctrl], "k", MenuAction::Format(Format::Link));
    bind(&[Ctrl, Shift], "3", MenuAction::Format(Format::Tag));
    bind(&[Ctrl], "r", MenuAction::Format(Format::Rule));
    bind(&[Ctrl, Shift], "q", MenuAction::Format(Format::Quote));
    // View
    bind(&[Ctrl, Shift], "1", MenuAction::ToggleNav);
    bind(&[Ctrl, Shift], "2", MenuAction::ToggleList);
    bind(&[Ctrl, Shift], "0", MenuAction::Solo);
    // Ctrl + plus is Ctrl + Shift + = on most keyboards; take every spelling.
    bind(&[Ctrl], "+", MenuAction::ZoomIn);
    bind(&[Ctrl, Shift], "+", MenuAction::ZoomIn);
    bind(&[Ctrl], "=", MenuAction::ZoomIn);
    bind(&[Ctrl], "-", MenuAction::ZoomOut);
    bind(&[Ctrl], "0", MenuAction::ZoomReset);
    bind(&[Alt], ",", MenuAction::Themes);
    bind(&[Ctrl, Shift], "m", MenuAction::ToggleMarkers);
    bind(&[Ctrl, Shift], "h", MenuAction::Shortcuts);
    binds
}

/// Where the caret goes after committing a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableMove {
    Stay,
    Down,
    Right,
    Left,
}

/// A column or row edge being dragged: which table, which line, and the
/// pointer position / size when the drag began.
#[derive(Debug, Clone, Copy)]
pub struct TableResize {
    block: usize,
    col: bool,
    index: usize,
    start: f32,
    size: f32,
}

/// A reference being picked into a formula: the table, the anchor cell,
/// and the draft text as it was when the pick began.
#[derive(Debug, Clone)]
pub struct TablePick {
    block: usize,
    anchor: (usize, usize),
    current: (usize, usize),
    base: String,
}

/// A fill-handle drag: the source cell and where the sweep has reached.
#[derive(Debug, Clone, Copy)]
pub struct TableFill {
    block: usize,
    from: (usize, usize),
    current: (usize, usize),
}

/// A cell-range selection being dragged (`active` once it left the
/// anchor cell — before that it is just a click waiting to happen).
#[derive(Debug, Clone, Copy)]
pub struct TableSel {
    block: usize,
    anchor: (usize, usize),
    current: (usize, usize),
    active: bool,
}

/// One undo step: the whole body plus where the caret was.
#[derive(Debug, Clone)]
pub struct Snapshot {
    body: String,
    focused: usize,
    cursor: text_editor::Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

/// A scrollable pane whose bar shows only while the pointer is over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollArea {
    Tags,
    Notes,
    Editor,
    Options,
    Picker,
    Backlinks,
}

/// A sidebar entry (a tag subtree or a spacer) being dragged to a new spot.
#[derive(Debug, Clone, Copy)]
pub struct TagDrag {
    /// Index into the root entry list.
    entry: usize,
    /// Where the button went down (window coords).
    start: Point,
    /// The pointer has travelled past `DRAG_THRESHOLD`: a drag, not a click.
    active: bool,
    /// Root entry the dragged one will sit before when dropped.
    target: Option<usize>,
}

/// A picture being dragged to a new line.
#[derive(Debug, Clone, Copy)]
pub struct ImageDrag {
    block: usize,
    /// Where the button went down (window coords).
    start: Point,
    /// The pointer has travelled past `DRAG_THRESHOLD`: a drag, not a click.
    active: bool,
    /// Body line the picture will sit before when dropped.
    target: Option<usize>,
}

/// A drag-resize in progress on an image block.
#[derive(Debug, Clone, Copy)]
pub struct Resize {
    block: usize,
    start_x: f32,
    start_w: u32,
}

/// A web link's preview: being fetched, ready (with its picture), or not available.
#[derive(Debug, Clone)]
pub enum PreviewState {
    Loading,
    Ready(crate::links::Preview, Option<widget::image::Handle>),
    Failed(String),
}

/// A processed image ready for display (or on its way).
#[derive(Debug, Clone)]
pub enum ImageState {
    Loading,
    Ready(widget::image::Handle, u32, u32),
    Ascii(String),
    Failed(String),
}

/// Markdown formatting actions offered by the dock.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Format {
    Bold,
    Italic,
    /// `==text==`: a highlighter band behind the words.
    Mark,
    /// Insert a table block at the end of the note.
    Table,
    Code,
    H1,
    H2,
    Bullet,
    Todo,
    Link,
    Tag,
    Rule,
    Quote,
}

impl Format {
    pub const ALL: [Format; 13] = [
        Format::Bold,
        Format::Italic,
        Format::Mark,
        Format::Code,
        Format::Table,
        Format::H1,
        Format::H2,
        Format::Bullet,
        Format::Todo,
        Format::Quote,
        Format::Link,
        Format::Tag,
        Format::Rule,
    ];

    fn key(self) -> &'static str {
        match self {
            Format::Bold => "bold",
            Format::Italic => "italic",
            Format::Code => "code",
            Format::H1 => "h1",
            Format::H2 => "h2",
            Format::Bullet => "bullet",
            Format::Todo => "todo",
            Format::Link => "link",
            Format::Tag => "tag",
            Format::Mark => "mark",
            Format::Table => "table",
            Format::Rule => "rule",
            Format::Quote => "quote",
        }
    }

    /// The dock drawing for this action, in the user's icon set.
    fn icon(self) -> DockIcon {
        match self {
            Format::Bold => DockIcon::Bold,
            Format::Italic => DockIcon::Italic,
            Format::Code => DockIcon::Code,
            Format::H1 => DockIcon::H1,
            Format::H2 => DockIcon::H2,
            Format::Bullet => DockIcon::Bullet,
            Format::Todo => DockIcon::Todo,
            Format::Link => DockIcon::Link,
            Format::Tag => DockIcon::Tag,
            Format::Rule => DockIcon::Rule,
            Format::Quote => DockIcon::Quote,
            Format::Mark => DockIcon::Mark,
            Format::Table => DockIcon::Table,
        }
    }

    fn label(self) -> String {
        match self {
            Format::Bold => fl!("dock-bold"),
            Format::Italic => fl!("dock-italic"),
            Format::Code => fl!("dock-code"),
            Format::H1 => fl!("dock-h1"),
            Format::H2 => fl!("dock-h2"),
            Format::Bullet => fl!("dock-bullet"),
            Format::Todo => fl!("dock-todo"),
            Format::Link => fl!("dock-link"),
            Format::Tag => fl!("dock-tag"),
            Format::Rule => fl!("dock-rule"),
            Format::Quote => fl!("dock-quote"),
            Format::Mark => fl!("dock-mark"),
            Format::Table => fl!("dock-table"),
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    Themes,
    Picker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
    NewNote,
    TrashNote,
    FocusSearch,
    Undo,
    Redo,
    NewFolder,
    AddImage,
    AttachFile,
    Pin,
    Format(Format),
    Shortcuts,
    Themes,
    ToggleMarkers,
    ToggleNav,
    ToggleList,
    Solo,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Quit,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
            MenuAction::Quit => Message::Quit,
            MenuAction::NewNote => Message::NewNote,
            MenuAction::Undo => Message::Undo,
            MenuAction::Redo => Message::Redo,
            MenuAction::NewFolder => Message::ToggleDock,
            MenuAction::AddImage => Message::PickImage,
            MenuAction::AttachFile => Message::PickFile,
            MenuAction::Pin => Message::TogglePin,
            MenuAction::Format(f) => Message::Format(*f),
            MenuAction::Shortcuts => Message::ToggleShortcuts,
            MenuAction::TrashNote => Message::TrashCurrent,
            MenuAction::FocusSearch => Message::FocusSearch,
            MenuAction::Themes => Message::ToggleContextPage(ContextPage::Themes),
            MenuAction::ToggleMarkers => Message::ToggleMarkers,
            MenuAction::ToggleNav => Message::ToggleNav,
            MenuAction::ToggleList => Message::ToggleList,
            MenuAction::Solo => Message::ToggleSolo,
            MenuAction::ZoomIn => Message::SizeStep(Pane::Editor, 1),
            MenuAction::ZoomOut => Message::SizeStep(Pane::Editor, -1),
            MenuAction::ZoomReset => Message::SizeReset(Pane::Editor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keys_resolve_in_every_spelling() {
        use cosmic::iced::keyboard::{Modifiers, key::Key};
        let binds = key_binds();
        let find = |mods: Modifiers, ch: &str| {
            binds
                .iter()
                .find(|(b, _)| b.matches(mods, &Key::Character(ch.into()), None))
                .map(|(_, a)| *a)
        };
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(find(Modifiers::CTRL, "+"), Some(MenuAction::ZoomIn));
        assert_eq!(find(ctrl_shift, "+"), Some(MenuAction::ZoomIn));
        assert_eq!(find(Modifiers::CTRL, "="), Some(MenuAction::ZoomIn));
        assert_eq!(find(Modifiers::CTRL, "-"), Some(MenuAction::ZoomOut));
        assert_eq!(find(Modifiers::CTRL, "0"), Some(MenuAction::ZoomReset));
        // The old binds around them are untouched.
        assert_eq!(find(ctrl_shift, "0"), Some(MenuAction::Solo));
        assert_eq!(find(Modifiers::CTRL, "1"), Some(MenuAction::Format(Format::H1)));
    }

    fn swept(text: &str, from: (usize, &str), to: (usize, &str), sweep: bool) -> String {
        use text_editor::{Cursor, Position};
        let mut c = crate::editor::Content::with_text(text);
        let lines: Vec<&str> = text.lines().collect();
        let col = |(l, needle): (usize, &str)| match needle {
            "$" => lines[l].len(),
            n => lines[l].find(n).expect("needle on the line"),
        };
        c.move_to(Cursor {
            position: Position { line: to.0, column: col(to) },
            selection: Some(Position { line: from.0, column: col(from) }),
        });
        assert!(mark_selection(&mut c, sweep));
        c.text()
    }

    #[test]
    fn the_pen_marks_whole_words_and_never_nests() {
        // Landed mid-word, overshot into a space: whole words, no spaces.
        assert_eq!(
            swept("• Minimalism mode today", (0, "malism"), (0, " today"), true),
            "• ==Minimalism mode== today\n"
        );
        // From the gutter: the typed bullet and list marker stay outside.
        assert_eq!(swept("• A drop down", (0, "•"), (0, "$"), true), "• ==A drop down==\n");
        assert_eq!(swept("- [ ] milk", (0, "-"), (0, "$"), true), "- [ ] ==milk==\n");
        assert_eq!(swept("## Plans", (0, "#"), (0, "$"), true), "## ==Plans==\n");
        // Across lines: a pair per line, blank lines left alone.
        assert_eq!(
            swept("just basic functions\n\n• A calendar", (0, "asic"), (2, "$"), true),
            "just ==basic functions==\n\n• ==A calendar==\n"
        );
        // Widening an existing highlight merges with it.
        assert_eq!(
            swept("oh yeah that's fi==xed!==", (0, "yeah"), (0, "$"), true),
            "oh ==yeah that's fixed!==\n"
        );
        // Sweeping highlighted text again rubs it out.
        assert_eq!(
            swept("a ==marked run== b", (0, "arked"), (0, " run"), true),
            "a marked run b\n"
        );
        // Right to left is the same sweep.
        assert_eq!(swept("one two three", (0, " three"), (0, "wo"), true), "one ==two== three\n");
    }

    #[test]
    fn marking_from_the_keyboard_keeps_the_exact_selection() {
        assert_eq!(swept("unhappy", (0, "happy"), (0, "$"), false), "un==happy==\n");
        assert_eq!(swept("a word here", (0, " word"), (0, "here"), false), "a ==word== here\n");
    }

    #[test]
    fn tag_tree_inserts_parents() {
        let tags = vec![
            ("work/incab".to_string(), 2),
            ("work".to_string(), 1),
            ("zed/a/b".to_string(), 1),
        ];
        let tree = tag_tree(&tags);
        assert_eq!(
            tree,
            vec![
                ("work".to_string(), 1),
                ("work/incab".to_string(), 2),
                ("zed".to_string(), 0),
                ("zed/a".to_string(), 0),
                ("zed/a/b".to_string(), 1),
            ]
        );
    }
}

/// Scroll the scrollable `target` just enough that `rect` (layout
/// coordinates, as a child of that scrollable sees them) sits inside its
/// viewport with `margin` above or below; no-op when it already does.
fn reveal_in_scrollable(
    target: widget::Id,
    rect: Rectangle,
    margin: f32,
) -> impl cosmic::iced::advanced::widget::Operation<()> {
    use cosmic::iced::advanced::widget::{Operation, operation::Scrollable};
    use cosmic::iced::widget::scrollable::AbsoluteOffset;
    struct Reveal {
        target: widget::Id,
        rect: Rectangle,
        margin: f32,
    }
    impl Operation<()> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            id: Option<&widget::Id>,
            bounds: Rectangle,
            content_bounds: Rectangle,
            translation: Vector,
            state: &mut dyn Scrollable,
        ) {
            if id != Some(&self.target) {
                return;
            }
            // Where the caret line sits in the (unscrolled) content.
            let top = self.rect.y - bounds.y;
            let bottom = top + self.rect.height;
            let view_top = translation.y;
            let view_bottom = translation.y + bounds.height;
            let max = (content_bounds.height - bounds.height).max(0.0);
            let y = if top - self.margin < view_top {
                (top - self.margin).max(0.0)
            } else if bottom + self.margin > view_bottom {
                (bottom + self.margin - bounds.height).min(max)
            } else {
                return;
            };
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some(y),
            });
        }
    }
    Reveal {
        target,
        rect,
        margin,
    }
}
