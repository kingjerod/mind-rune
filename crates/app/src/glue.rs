//! The window's state, and the glue between it and the core: effects are
//! carried out here and the backend's messages are taken in here, so the
//! views only read `Ui` and call the `App`'s intents.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mr_agent::{Agent, AgentError, FakeAgent, ImageData, RequestId};
#[cfg(test)]
use mr_core::EntryMut;
use mr_core::{
    App, Backend, Busy, DEFAULT_WIDTH, Deleting, Effect, FromBackend, GroupId, ItemBody, ItemId,
    Launch, Leaving, Line, Phase, Place, Prepared, RequestState, SAVED_VERSION, Saved, Screen,
    Session, SessionKey, SidebarView, Source, Spot, Standing, ToBackend, Width,
};
use rune::prefs::Prefs;
use rune::runtime::Waker;
use rune::{Image, ImageCache, PointerPress, TextEdit, Tx};
use rune_kit::ImageZoom;
use rune_kit::shell::PaneState;

use crate::demo;
use crate::notify::Note;
use crate::view;

/// The settings key the conversation's width is kept under.
const WIDTH_KEY: &str = "conversation_width";

/// The settings key what is kept between runs is written under, as one [`Saved`].
pub(crate) const STATE_KEY: &str = "state";

/// The settings key a value under [`STATE_KEY`] that this build cannot read is copied to, as it
/// was, before this build writes its own: so that a newer build's state survives a run of this
/// one.
pub(crate) const UNREAD_KEY: &str = "state.unread";

/// How often the main screen polls on its own: the app's clock is brought up to date, so a new
/// listing and a usage retry go out when due, and a usage window's reset wording is redrawn.
const TICK: Duration = Duration::from_secs(30);

/// How often the demo window polls, so the fake's scripted replies play on their own.
const DEMO_TICK: Duration = Duration::from_millis(50);

/// How long the folder error stays under "New session" when no new try takes it away: long
/// enough to read a line with a path in it.
pub const FOLDER_ERROR_HOLD: Duration = Duration::from_secs(8);

/// Which of the input box's pickers has its list open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Model,
    Effort,
    Mode,
}

/// One question's answer in the making: the options chosen, in the order they were chosen,
/// and the "Something else" field.
#[derive(Clone, Default, PartialEq, Hash)]
pub struct QuestionDraft {
    pub chosen: Vec<String>,
    pub other: TextEdit,
}

/// What the sidebar's menu is the menu of.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuTarget {
    /// A session's row.
    Row(SessionKey),
    /// A group's header.
    Group(GroupId),
    /// The "Hidden groups" line: one item per hidden group.
    HiddenGroups,
    /// A session's row in the Deleted view.
    Deleted(SessionKey),
    /// A group's header in the Deleted view.
    DeletedGroup(GroupId),
}

/// A question about deleting that waits for its answer.
#[derive(Clone, Debug, PartialEq)]
pub enum DeleteAsk {
    /// Deleting a session that would cut something off.
    Session(SessionKey),
    /// Deleting a group and every session of it.
    Group(GroupId),
    /// Removing everything in the Deleted view for good.
    All,
}

/// What the window's one name field edits.
#[derive(Clone, Debug, PartialEq)]
pub enum Renaming {
    /// A group, in its header in the sidebar.
    Group(GroupId),
    /// A session, in its sidebar row.
    Row(SessionKey),
    /// A session, in its tile's header.
    Header(SessionKey),
}

/// Where a drag in [`Ui::drag`] would go if it were let go there.
#[derive(Clone, Debug, PartialEq)]
pub enum DropTarget {
    /// This edge of the tile row, as `Tiles::edge_at` counts them: a new tile there for a
    /// row's drag, the tile moved there for a header's.
    Edge(usize),
    /// In place of this tile's session.
    Tile(SessionKey),
    /// Into this group.
    Group(GroupId),
    /// Over this tile, in its column: a new tile there for a row's drag, the dragged tile
    /// there for a header's.
    Above(SessionKey),
    /// Under this tile, in its column.
    Below(SessionKey),
}

/// What a drag in [`Ui::drag`] took hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragFrom {
    /// A sidebar row.
    Row,
    /// A tile's header.
    Header,
}

/// A sidebar row or a tile's header on its way: past the slop, not yet let go.
#[derive(Clone, Debug, PartialEq)]
pub struct RowDrag {
    /// The session of the row, or of the tile.
    pub key: SessionKey,
    /// The pointer, in window points.
    pub at: [f32; 2],
    pub over: Option<DropTarget>,
    /// Whether the drag heard the last pointer move, and how many it missed in a row.
    pub heard: bool,
    pub missed: u8,
    pub from: DragFrom,
}

/// The sidebar's menu, open: there is one at a time, for the whole window.
#[derive(Clone, Debug, PartialEq)]
pub struct SideMenu {
    pub target: MenuTarget,
    /// Where it opens: where the secondary press landed, or the bottom-left of the focused row
    /// for the Menu key and Shift+F10. A press on the "Hidden groups" line gives no point: that
    /// menu opens under its line (see `view::sidebar::menu`).
    pub at: PointerPress,
    /// A row's "Move to group" submenu is open.
    pub sub_open: bool,
}

/// How one session is shown: everything the window remembers about it that the core does not.
#[derive(Clone)]
pub struct SessionView {
    /// What is typed in its input box.
    pub draft: TextEdit,
    pub open_picker: Option<Picker>,
    /// Items the user has opened or closed, against each kind's default.
    pub toggled: HashSet<ItemId>,
    /// Items whose long output the user asked to see whole.
    pub whole: HashSet<ItemId>,
    /// What the user has chosen and typed so far in each waiting question, one draft per
    /// question. A request's drafts are dropped as soon as it stops waiting in this session.
    pub answers: HashMap<RequestId, Vec<QuestionDraft>>,
    /// The conversation is scrolled to its end.
    pub following: bool,
    /// The first item on screen as the conversation was last scrolled: where it is shown from
    /// again while it is not following its end.
    pub first_seen: Option<ItemId>,
    /// The session's `generation` these were made for.
    pub generation: u64,
    /// The folder a message could not wake the session in, because it is no longer there:
    /// said under the input box until the next try.
    pub folder_gone: Option<PathBuf>,
    /// Why the file picker of the attach button could not be used: said under the input box
    /// until the picker's next answer.
    pub attach_error: Option<String>,
}

impl SessionView {
    /// A session not shown yet: nothing typed, every row as its kind starts, at the end.
    fn new(generation: u64) -> SessionView {
        SessionView {
            draft: TextEdit::new(""),
            open_picker: None,
            toggled: HashSet::new(),
            whole: HashSet::new(),
            answers: HashMap::new(),
            following: true,
            first_seen: None,
            generation,
            folder_gone: None,
            attach_error: None,
        }
    }

    /// The session is no longer drawn: its fields give up the keyboard focus ([`let_go`]),
    /// which a field left focused would take back whenever it is drawn again, and its picker
    /// list shuts.
    fn leave(&mut self) {
        self.open_picker = None;
        let_go(&mut self.draft);
        for draft in self.answers.values_mut().flatten() {
            let_go(&mut draft.other);
        }
    }
}

/// A field that is no longer drawn gives up the keyboard focus. One that had it is made anew,
/// with the same text and caret and no undo history, so that it can ask for the focus again.
///
/// Rune keeps a field unmounted while it had the focus as one that may come back, and knows it
/// by its undo history: drawn again while another field holds the focus, such a field asking
/// for it is taken for its old flag and not heard (`sync_requested_focus`, in
/// `crates/rune/src/runtime/input/focus.rs` in the Rune checkout).
fn let_go(edit: &mut TextEdit) {
    if !edit.focused {
        return;
    }
    let mut fresh = TextEdit::new(edit.text.clone());
    fresh.anchor = edit.anchor;
    fresh.focus = edit.focus;
    *edit = fresh;
}

/// The picture open large over the window (`view::viewer`).
pub struct Viewing {
    pub picture: u64,
    pub key: SessionKey,
    /// The full image, once its bytes are there. It may still be decoding.
    pub full: Option<Image>,
    pub failed: bool,
    /// The full image was seen decoded ([`full_made`]), so the view has drawn it.
    made: bool,
}

pub struct Ui {
    pub app: App,
    pub agent: Arc<dyn Agent>,
    /// Present in the demo window and in tests: the agent, as the fake it is.
    pub fake: Option<FakeAgent>,
    pub backend: Option<Backend>,
    pub prefs: Prefs,
    pub sidebar: PaneState,
    /// The right panel: 300 points wide at first, 240 to 480, hidden at first.
    pub panel: PaneState,
    /// How each session shown in this run is shown, by its key. A session that is gone loses
    /// its view at the next [`run`].
    pub views: HashMap<SessionKey, SessionView>,
    /// Where the wall clock is read from: `app.now_unix` is set from it on every poll.
    pub unix_clock: fn() -> i64,
    /// Whether a folder is there, asked when a message would wake a resting session in it.
    /// The demo's folders are made up, so the demo says yes to every one.
    pub folder_exists: fn(&Path) -> bool,
    /// Whether a folder's branch is read from the disk. The demo's folders are made up, so the
    /// demo reads none: a scene gives its answer itself.
    pub reads_branches: bool,
    /// Messages for the backend that arrived before `setup` started it.
    waiting: Vec<ToBackend>,
    /// The `now` of the last `poll`.
    last_poll: Option<f64>,
    /// In the demo, the seconds the fake has been advanced: what events are stamped with.
    pub(crate) fake_time: f64,
    /// What [`view_of`] lends for a session that has no view: one not shown yet.
    blank: SessionView,
    /// The sessions in tiles as of the last [`run`].
    was_tiled: Vec<SessionKey>,
    /// The tiles' placement as of the last [`run`] (`Tiles::placement`).
    was_placed: u64,
    /// The tile row as last laid out, in window points: x, y, width, height.
    pub row_rect: Option<[f32; 4]>,
    /// The shares of the two tiles beside the divider being dragged, as they were at its press:
    /// what a cancelled drag puts back. Forgotten when the tiles' placement changes ([`run`]).
    pub divider_press: Option<[f32; 2]>,
    /// The split of the column whose line is being dragged, as it was at the press. Forgotten
    /// as `divider_press` is.
    pub split_press: Option<f32>,
    /// The sidebar's menu, while it is open.
    pub side_menu: Option<SideMenu>,
    /// The sidebar row or the tile header being dragged (`view::drag`).
    pub drag: Option<RowDrag>,
    /// The tile files dragged from another app are held over, which shows its drop veil
    /// (`view::tiles`). Forgotten when its tile goes or its session takes no message ([`run`]).
    pub file_drop: Option<SessionKey>,
    /// The row's press that may follow a drag's release is not a click (`view::drag::row`).
    ///
    /// Set at the release of every row's drag that went past the slop, whatever is under the
    /// pointer; a header's drag never sets it.
    /// Rune queues the row's press, when it runs one, right after that release, from the same
    /// pointer release (`release`, in `crates/rune/src/runtime/input/pointer.rs` in the Rune
    /// checkout), and a frame dispatches all the input queued before it, so that press comes in
    /// the same frame and takes the flag. [`poll`] clears it: it runs before the input of every
    /// later frame (or a timer's poll between frames stands for it), so no later press is
    /// swallowed. Only a press of a row queued after the release and before that frame (an
    /// assistive tool's activation within the same frame) could still be taken.
    pub swallow_row_press: bool,
    /// The row of a session in another tile that the list's arrow keys passed onto, with the
    /// tiles' placement then (`Tiles::placement`): it is the list's selected row until a row
    /// is pressed or a tile comes, goes or takes the focus (`view::sidebar::selected_row`).
    pub passed_row: Option<(SessionKey, u64)>,
    /// The tile whose text field held the keyboard at the end of the last [`run`], or that the
    /// glue itself last gave the keyboard to ([`type_in`]): how [`run`] tells the keyboard
    /// coming into a tile, which is the user's act, from the focus handed back to the tile
    /// that had it all along.
    typed_in: Option<SessionKey>,
    /// The name being edited, and the field it is edited in: one at a time, in the window.
    pub renaming: Option<Renaming>,
    pub rename: TextEdit,
    /// The folder menu is open, for this group when one is named.
    pub folder_menu: Option<Option<GroupId>>,
    /// The open folder menu starts its session to the side (Ctrl+Shift+N, or Ctrl held on a
    /// custom group's "+"): a folder chosen in it, or in the folder picker it opens, opens a tile
    /// right of the focused one. It goes when the menu shuts.
    pub folder_side: bool,
    /// The folders the open folder menu offers: the app's choices that were folders on the disk
    /// as it opened (`view::folder::ask`), so it reads the disk once per opening.
    pub folder_offers: Vec<PathBuf>,
    /// Why the folder picker could not be used, or that a folder chosen is gone: shown under the
    /// button until the next try, or until its time is up ([`FOLDER_ERROR_HOLD`]).
    pub folder_error: Option<String>,
    /// When the folder error goes by itself, on the frames' clock: set by the first poll that
    /// sees it, and none again once a new one is set (`view::folder`).
    pub folder_error_until: Option<f64>,
    /// Where the chips' thumbnails and the messages' pictures are made: set by [`setup`], with
    /// the window's waker. Until then, and in a view test that does not set one, none is made.
    pub images: Option<ImageCache>,
    /// The drawn picture of each picture id that has one: a chip's thumbnail by the chip's id,
    /// which a message sent with the chip keeps, and a stored message's picture by its own id.
    /// Made as the chip's answer or the picture is taken in ([`take_in`]), dropped once no
    /// session needs the id ([`tidy_views`]).
    pub thumbs: HashMap<u64, Image>,
    /// The ids in `thumbs` whose handle may not have been made when it was put there. A
    /// picture's width comes from its handle's size, so the poll after one is made draws again
    /// ([`handles_made`]).
    pub unready: Vec<u64>,
    /// The attach button's file picker is open: set at the press that opened it, cleared by its
    /// answer. A press while it is open does nothing.
    pub file_picker_open: bool,
    /// Chips whose answer said they cannot be sent, to be scrolled into view in their row by
    /// the next [`poll`].
    pub reveal_chips: Vec<u64>,
    /// The programs were told to end ([`quitting`]); set once.
    pub quitting: bool,
    /// The question whether to quit shows (`view::quit`): something would be cut off.
    pub quit_ask: bool,
    /// The question about deleting that shows (`view::deleted`), if one does.
    pub delete_ask: Option<DeleteAsk>,
    /// The picture open large over the window, if one is ([`open_picture`]).
    pub viewing: Option<Viewing>,
    /// How far the open picture is zoomed and panned: the kit's viewer keeps it here.
    pub zoom: ImageZoom,
    /// Tells the tray how the sessions stand; there only where a tray was started.
    pub tray: Option<Sender<Standing>>,
    /// The standing the tray was last told ([`poll`]).
    tray_said: Standing,
    /// Tells the desktop's notifications what to show and take back; there only where a
    /// notifier was started.
    pub notifier: Option<Sender<Note>>,
}

impl Ui {
    pub fn new(agent: Arc<dyn Agent>, fake: Option<FakeAgent>, launch: Launch, prefs: Prefs) -> Ui {
        let (ui, warning) = Ui::open(agent, fake, launch, prefs);
        if let Some(warning) = warning {
            eprintln!("{warning}");
        }
        ui
    }

    /// As [`Ui::new`], which says the line this returns, when there is one, on stderr. What was
    /// kept from the last run is read from `prefs` here, in place of `launch.saved`: the
    /// sidebar's width and whether it is collapsed are applied to the sidebar, the right panel's
    /// width and whether it shows to the panel, the rest goes to the app.
    pub(crate) fn open(
        agent: Arc<dyn Agent>,
        fake: Option<FakeAgent>,
        mut launch: Launch,
        mut prefs: Prefs,
    ) -> (Ui, Option<String>) {
        let (saved, warning) = read_state(&mut prefs);
        let mut sidebar = PaneState::new(260.0).range(200.0, 420.0);
        if let Some(width) = saved.sidebar_width {
            sidebar = sidebar.with_width(width);
        }
        if saved.sidebar_collapsed {
            sidebar = sidebar.collapsed();
        }
        let mut panel = PaneState::new(300.0).range(240.0, 480.0);
        if let Some(width) = saved.panel_width {
            panel = panel.with_width(width);
        }
        if !saved.panel_open {
            panel = panel.collapsed();
        }
        launch.saved = saved;
        let width = prefs
            .get::<Width>(WIDTH_KEY)
            .unwrap_or(Width::Points(DEFAULT_WIDTH));
        let unix_clock: fn() -> i64 = system_unix;
        let (app, effects) = App::new(agent.capabilities(), width, launch, unix_clock());
        let mut ui = Ui {
            app,
            agent,
            fake,
            backend: None,
            prefs,
            sidebar,
            panel,
            views: HashMap::new(),
            unix_clock,
            folder_exists: Path::is_dir,
            reads_branches: true,
            waiting: Vec::new(),
            last_poll: None,
            fake_time: 0.0,
            blank: SessionView::new(0),
            was_tiled: Vec::new(),
            was_placed: 0,
            row_rect: None,
            divider_press: None,
            split_press: None,
            side_menu: None,
            drag: None,
            file_drop: None,
            swallow_row_press: false,
            passed_row: None,
            typed_in: None,
            renaming: None,
            rename: TextEdit::new(""),
            folder_menu: None,
            folder_side: false,
            folder_offers: Vec::new(),
            folder_error: None,
            folder_error_until: None,
            images: None,
            thumbs: HashMap::new(),
            unready: Vec::new(),
            file_picker_open: false,
            reveal_chips: Vec::new(),
            quitting: false,
            quit_ask: false,
            delete_ask: None,
            viewing: None,
            zoom: ImageZoom::new(),
            tray: None,
            tray_said: Standing::default(),
            notifier: None,
        };
        run(&mut ui, effects);
        (ui, warning)
    }

    /// Forgets the messages waiting for `setup`, so they are never carried out.
    /// Only for the demo's `checking` scene, whose account check never comes back.
    pub(crate) fn forget_waiting(&mut self) {
        self.waiting.clear();
    }

    /// The session in the focused tile. The demo and tests read it; a view draws the session it
    /// is given.
    pub fn focused_session(&self) -> Option<&Session> {
        self.app.focused_entry().map(|entry| &entry.session)
    }

    /// The focused session's entry, lent out to change.
    #[cfg(test)]
    pub fn focused_entry_mut(&mut self) -> Option<EntryMut<'_>> {
        let key = self.app.focused()?;
        self.app.sessions.get_mut(key)
    }
}

/// How `key`'s session is shown; for a session with no view yet, the one a session not shown
/// yet starts with.
pub fn view_of(ui: &Ui, key: SessionKey) -> &SessionView {
    ui.views.get(&key).unwrap_or(&ui.blank)
}

/// How `key`'s session is shown, lent out to change; made when it has no view yet.
pub fn view_mut(ui: &mut Ui, key: SessionKey) -> &mut SessionView {
    let generation = ui
        .app
        .sessions
        .get(key)
        .map_or(0, |entry| entry.session.generation);
    ui.views
        .entry(key)
        .or_insert_with(|| SessionView::new(generation))
}

/// What becomes of the session `place` would take out of its tile: kept while text is typed
/// in its input box or it has chips, else forgotten when it is empty. `Side`, `At`, `Above` and
/// `Below` take none out, but `Side` swaps the focused tile's session when there is no room, so
/// it is read as `Focused`.
pub fn leaving(ui: &Ui, place: Place) -> Leaving {
    let out = match place {
        Place::Focused | Place::Side => ui.app.focused(),
        Place::Tile(key) => Some(key),
        Place::At(_) | Place::Above(_) | Place::Below(_) => None,
    };
    let typed = out.is_some_and(|left| {
        let chips = ui
            .app
            .sessions
            .get(left)
            .is_some_and(|entry| !entry.session.chips.is_empty());
        chips
            || view_of(ui, left)
                .draft
                .text
                .chars()
                .any(|c| !c.is_whitespace())
    });
    if typed {
        Leaving::Keep
    } else {
        Leaving::ForgetIfEmpty
    }
}

/// Shows `key`'s session at `place`. One shown for the first time starts at its end; one shown
/// again keeps its draft and is scrolled back to where it was. The session taken out of its
/// tile stays while text is typed in it ([`leaving`]). With `focus`, its input box takes the
/// keyboard focus, as when the user picked it with a click; without, the focus stays where it
/// is (on the sidebar's list, say, whose arrow keys move from session to session).
pub fn show(ui: &mut Ui, tx: &mut Tx, key: SessionKey, place: Place, focus: bool) {
    let leaving = leaving(ui, place);
    show_by(ui, tx, key, focus, |app| app.show(key, place, leaving));
}

/// [`show`] by `intent`, the `App`'s intent that shows `key`'s session.
fn show_by(
    ui: &mut Ui,
    tx: &mut Tx,
    key: SessionKey,
    focus: bool,
    intent: impl FnOnce(&mut App) -> Vec<Effect>,
) {
    let tiled = ui.app.tiles.contains(key);
    let effects = intent(&mut ui.app);
    if ui.app.focused() == Some(key) {
        // At once, as a tab shows: the conversation left does not fade out over this one, and
        // the tiles take their new widths in the same frame.
        tx.instant();
        let again = ui.views.contains_key(&key);
        if focus {
            type_in(ui, key);
        }
        let view = view_mut(ui, key);
        // A new viewport opens at its end, which is where a following one is anyway.
        if !tiled
            && again
            && !view.following
            && let Some(first) = view.first_seen
        {
            tx.scroll_into_view_in(view::conversation_key(key), view::row_key(first));
        }
    }
    run(ui, effects);
}

/// A notification of this session was pressed: as a press on its row ([`show`]), it shows in
/// the focused tile, or its tile takes the focus, and its input box takes the keyboard.
pub fn alert_pressed(ui: &mut Ui, tx: &mut Tx, key: SessionKey) {
    let leaving = leaving(ui, Place::Focused);
    show_by(ui, tx, key, true, |app| app.alert_pressed(key, leaving));
}

/// `key`'s tile takes the focus; with `typing`, its input box takes the keyboard focus.
/// Without, while a field of another tile holds the keyboard focus, the focus stays there
/// ([`run`]).
///
/// `typing` is a focus asked for on purpose (F6, a press on the tile). When the tile's box
/// cannot take the keyboard (its session has ended), the tile is focused all the same and
/// no field keeps the keyboard: the one that had it gives it up, or [`run`] would hand the
/// focus back to its tile.
pub fn focus_tile(ui: &mut Ui, key: SessionKey, typing: bool) {
    // The box asks first, so that `run` finds the keyboard going to the tile focused.
    if typing {
        take_keyboard(ui, key);
    }
    let effects = ui.app.focus_tile(key);
    run(ui, effects);
}

/// Attaches `sources` to `key`'s session and runs what that asks for. The last chip added is
/// scrolled into view in its row, which never wraps.
pub fn attach(ui: &mut Ui, tx: &mut Tx, key: SessionKey, sources: Vec<Source>) {
    let effects = ui.app.attach(key, sources);
    let added = effects.iter().rev().find_map(|effect| match effect {
        Effect::Prepare { chip, .. } => Some(*chip),
        _ => None,
    });
    if let Some(chip) = added {
        tx.scroll_into_view(view::chip_key(chip));
    }
    run(ui, effects);
}

/// The keyboard focus goes to `key`'s input box, for a tile focused on purpose
/// ([`focus_tile`]); when the box cannot take it, no field keeps it.
fn take_keyboard(ui: &mut Ui, key: SessionKey) {
    if type_in(ui, key) {
        return;
    }
    for view in ui.views.values_mut() {
        view.draft.focused = false;
        for draft in view.answers.values_mut().flatten() {
            draft.other.focused = false;
        }
    }
    // A name being edited is kept as the field lets go (`view::sidebar::rename_watch`).
    ui.rename.focused = false;
}

/// The tile one of whose text fields has the keyboard focus: its input box, the "Something
/// else" field of one of its questions, or the field its header's name is edited in, as Rune
/// last wrote the focus into them. The focused tile is asked first, so a flag left behind in
/// another tile can never take the focus away from it. The field of a name edited in a
/// sidebar row belongs to no tile.
pub fn typing_in(ui: &Ui) -> Option<SessionKey> {
    let typing = |key: &SessionKey| {
        let view = view_of(ui, *key);
        view.draft.focused
            || view.answers.values().flatten().any(|d| d.other.focused)
            || (ui.rename.focused && ui.renaming == Some(Renaming::Header(*key)))
    };
    let focused = ui.app.focused().filter(typing);
    focused.or_else(|| ui.app.tiles.keys().into_iter().find(typing))
}

/// The input box of `key`'s session takes the keyboard focus, while the session is in a tile
/// and its box takes text: not once it has ended, when the box drawn is a disabled one. False
/// when it does not.
pub fn type_in(ui: &mut Ui, key: SessionKey) -> bool {
    let typing = ui.app.tiles.contains(key)
        && ui
            .app
            .sessions
            .get(key)
            .is_some_and(|entry| !ended(&entry.session));
    if typing {
        view_mut(ui, key).draft.focused = true;
        // The glue's own doing, not the keyboard coming there by the user's act ([`run`]).
        ui.typed_in = Some(key);
    }
    typing
}

fn ended(session: &Session) -> bool {
    matches!(session.phase, Phase::Ended { .. })
}

/// Closes `key`'s tile; its session leaves as `leaving(ui, Place::Tile(key))` says. The tiles
/// take their new widths at once. When the tile closed was the focused one, the input box of the
/// tile focused now takes the keyboard focus; otherwise the focus stays where it is.
///
/// A name being edited in its header goes with it, dropped.
pub fn close_tile(ui: &mut Ui, tx: &mut Tx, key: SessionKey) {
    if ui.renaming == Some(Renaming::Header(key)) {
        end_rename(ui, false);
    }
    let was_focused = ui.app.focused() == Some(key);
    let effects = ui.app.close_tile(key, leaving(ui, Place::Tile(key)));
    tx.instant();
    run(ui, effects);
    if was_focused && let Some(now) = ui.app.focused() {
        type_in(ui, now);
    }
}

/// Moves `key`'s tile to `spot`, at once, with the keyboard in its input box when `typing`.
///
/// The tile takes the focus (`App::move_tile`; `Tiles::move_to` says what its width becomes).
/// With `typing`, the keyboard goes to its box as [`focus_tile`] puts it there; without, it
/// stays where it is. Nothing in the tile is made anew: the tiles are keyed, so Rune moves the
/// tile's elements with it, and a field in it keeps the keyboard and what was typed. Nothing
/// happens when nothing would move.
pub fn move_tile(ui: &mut Ui, tx: &mut Tx, key: SessionKey, spot: Spot, typing: bool) {
    if !ui.app.can_move_tile(key, spot) {
        return;
    }
    tx.instant();
    // The box asks first, so that `run` finds the keyboard going to the tile focused.
    if typing {
        take_keyboard(ui, key);
    }
    let effects = ui.app.move_tile(key, spot);
    run(ui, effects);
}

/// Starts editing the name of `what`: the field holds its name (a session's title, empty while
/// it has none), all selected, and asks for the keyboard focus. The name being edited before,
/// if any, is kept first: one name is edited at a time in the window.
pub fn start_rename(ui: &mut Ui, what: Renaming) {
    end_rename(ui, true);
    let name = match &what {
        Renaming::Group(id) => ui.app.groups.name(id),
        Renaming::Row(key) | Renaming::Header(key) => ui
            .app
            .sessions
            .get(*key)
            .and_then(|entry| entry.session.title.clone())
            .unwrap_or_default(),
    };
    if let Renaming::Header(key) = what {
        // The press that started it asked for the keyboard in the tile's input box.
        view_mut(ui, key).draft.focused = false;
    }
    let mut edit = TextEdit::new(name);
    edit.anchor = 0;
    edit.focused = true;
    ui.rename = edit;
    ui.renaming = Some(what);
}

/// Ends the rename going on, if any. With `keep`, the name typed is given: to a group unless it
/// is the name it has; to a session through `App::rename`, which leaves an empty name or the
/// title it has alone.
pub fn end_rename(ui: &mut Ui, keep: bool) {
    let Some(what) = ui.renaming.take() else {
        return;
    };
    let edit = std::mem::replace(&mut ui.rename, TextEdit::new(""));
    if !keep {
        return;
    }
    let effects = match what {
        Renaming::Group(id) if edit.text.trim() != ui.app.groups.name(&id) => {
            ui.app.rename_group(&id, &edit.text)
        }
        Renaming::Group(_) => return,
        Renaming::Row(key) | Renaming::Header(key) => ui.app.rename(key, &edit.text),
    };
    run(ui, effects);
}

/// Return (`keep`) or Escape in the name field: the rename ends, and after one in a header the
/// keyboard focus goes to that tile's input box.
pub fn finish_rename(ui: &mut Ui, keep: bool) {
    let header = match ui.renaming {
        Some(Renaming::Header(key)) => Some(key),
        _ => None,
    };
    end_rename(ui, keep);
    if let Some(key) = header {
        focus_tile(ui, key, true);
    }
}

/// What was kept under [`STATE_KEY`]: the default when nothing is, and also, with a line to say
/// so, when what is there cannot be read as a [`Saved`] or is of another version. What could
/// not be read is first copied, as it was, to [`UNREAD_KEY`], since this build's first save
/// will write over it.
fn read_state(prefs: &mut Prefs) -> (Saved, Option<String>) {
    if !prefs.contains(STATE_KEY) {
        return (Saved::default(), None);
    }
    if let Some(saved) = prefs.get::<Saved>(STATE_KEY).map(Saved::checked)
        && saved.version == SAVED_VERSION
    {
        return (saved, None);
    }
    let raw: Option<serde_json::Value> = prefs.get(STATE_KEY);
    let kept = raw.map_or(Ok(()), |raw| prefs.set(UNREAD_KEY, &raw));
    let warning = match kept {
        Ok(()) => "mindrune: the saved state could not be read; starting from nothing".into(),
        Err(error) => format!(
            "mindrune: the saved state could not be read, nor kept aside ({error}); starting \
             from nothing"
        ),
    };
    (Saved::default(), Some(warning))
}

fn system_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `AppSpec::setup`: makes the thumbnails' cache and starts the backend with the window's waker,
/// and carries out what was waiting. A second call makes the cache anew, with the waker it is
/// given, and leaves the backend alone, so a scene that started its own backend keeps it and
/// the window's waker still hears the thumbnails made after.
pub fn setup(ui: &mut Ui, waker: Waker) {
    ui.images = Some(ImageCache::new(waker.clone()));
    if ui.backend.is_some() {
        return;
    }
    let backend = Backend::start(ui.agent.clone(), waker);
    for message in ui.waiting.drain(..) {
        backend.send(message);
    }
    ui.backend = Some(backend);
}

/// `AppSpec::poll`, which also runs while no window shows: nothing here needs a frame. The
/// app's clock is brought up to date first, so what is taken in after is stamped with it.
/// Tiles that come or go with what was taken in (a row restored, a session gone) do so at once,
/// as when the user opens or closes one.
pub fn poll(ui: &mut Ui, now: f64, tx: &mut Tx) -> bool {
    // A press that did not follow the release that set it never comes now. Nothing shows it.
    ui.swallow_row_press = false;
    let tiled = ui.was_tiled.clone();
    let since = ui.last_poll.map_or(0.0, |last| (now - last).max(0.0));
    ui.last_poll = Some(now);
    let minute = ui.app.now_unix / 60;
    let effects = ui.app.tick((ui.unix_clock)());
    run(ui, effects);
    let mut changed = false;
    let usage = &ui.app.usage;
    if usage.session.is_some() || usage.weekly.is_some() {
        changed |= ui.app.now_unix / 60 != minute;
    }
    if ui.app.screen == Screen::Main {
        tx.wake_after(TICK);
    }
    let stamp = if ui.fake.is_some() {
        changed |= demo::pump(ui, since as f32);
        tx.wake_after(DEMO_TICK);
        ui.fake_time
    } else {
        now
    };
    changed |= take_in(ui, stamp);
    changed |= folder_error_times_out(ui, now, tx);
    tell_tray(ui);
    changed |= handles_made(ui);
    changed |= full_made(ui);
    if ui.viewing.is_some() {
        changed |= ui.zoom.poll(now, tx);
    }
    for chip in ui.reveal_chips.drain(..) {
        tx.scroll_into_view(view::chip_key(chip));
    }
    if ui.was_tiled != tiled {
        tx.instant();
        changed = true;
    }
    changed
}

/// Takes the folder error away once it has been there for [`FOLDER_ERROR_HOLD`], and wakes
/// the window for that. It is about one try, and nothing else the user does takes it away, so
/// without this it stayed for as long as no new session was asked for. True when it went.
fn folder_error_times_out(ui: &mut Ui, now: f64, tx: &mut Tx) -> bool {
    if ui.folder_error.is_none() {
        ui.folder_error_until = None;
        return false;
    }
    let until = *ui
        .folder_error_until
        .get_or_insert(now + FOLDER_ERROR_HOLD.as_secs_f64());
    if now < until {
        tx.wake_after(Duration::from_secs_f64(until - now));
        return false;
    }
    ui.folder_error = None;
    ui.folder_error_until = None;
    true
}

/// Tells the notifier what to show or take back. Without one, the note is dropped; so is one
/// for a notifier that has ended.
fn tell_notifier(ui: &Ui, note: Note) {
    if let Some(notifier) = &ui.notifier {
        let _ = notifier.send(note);
    }
}

/// Tells the tray how the sessions stand when that changed since it was last told. Nothing
/// is said, or remembered as said, while there is no tray to tell; a tray that is gone hears
/// nothing.
fn tell_tray(ui: &mut Ui) {
    let Some(tray) = &ui.tray else {
        return;
    };
    let standing = ui.app.standing();
    if standing != ui.tray_said {
        let _ = tray.send(standing);
        ui.tray_said = standing;
    }
}

/// Carries effects out: backend messages are sent, the width is saved, and so is the state kept
/// between runs, with the widths of the sidebar and the right panel and whether each shows. A
/// look in a folder that is not there runs in the home folder. Messages for the backend wait
/// until `setup` has started it.
///
/// Every change to the `App` (an intent, or a message taken in) is followed by a call to this,
/// so it brings the sidebar's model up to date (`App::settle`, which also takes out the tiles
/// of sessions that are gone, and gives the notifications due and to take back: they are
/// carried out with the rest) and ends by bringing the sessions' views up to date with it
/// ([`tidy_views`]).
///
/// Last, the rule of ruling R7: while a text field of a tile holds the keyboard focus, that
/// tile is the focused one. An intent on a session in another tile (a request button, a row's
/// toggle: controls that take no keyboard focus) focused that other tile; the focus goes back
/// to the tile the user types in here, in the same change, so no outline or row flashes, and
/// without counting as the user's act on that tile's session (`App::return_focus`). The
/// keyboard coming into another tile's field since the last run (a click into its box) is
/// their act: that tile is focused as a press on it focuses it (`App::focus_tile`). A tile
/// focused on purpose whose box cannot take the keyboard leaves no field holding it
/// ([`focus_tile`]), so nothing is given back.
pub fn run(ui: &mut Ui, effects: Vec<Effect>) {
    let alerts = ui.app.settle();
    for effect in effects.into_iter().chain(alerts) {
        let message = match effect {
            Effect::SaveWidth(width) => {
                if let Err(error) = ui.prefs.set(WIDTH_KEY, &width) {
                    eprintln!("mindrune: could not save the conversation width: {error}");
                }
                continue;
            }
            Effect::SaveState(mut saved) => {
                saved.sidebar_width = Some(ui.sidebar.width());
                saved.sidebar_collapsed = !ui.sidebar.is_open();
                saved.panel_width = Some(ui.panel.width());
                saved.panel_open = ui.panel.is_open();
                if let Err(error) = ui.prefs.set(STATE_KEY, &*saved) {
                    eprintln!("mindrune: could not save the groups and recent folders: {error}");
                }
                continue;
            }
            Effect::CheckAccount => ToBackend::CheckAccount,
            Effect::Open { open, spec } => ToBackend::Open { open, spec },
            Effect::Send { open, command } => ToBackend::Send { open, command },
            Effect::ListSessions { since_unix } => ToBackend::ListSessions { since_unix },
            Effect::ReadHistory { key, id, max_items } => {
                ToBackend::ReadHistory { key, id, max_items }
            }
            Effect::DropHistory { key } => ToBackend::DropHistory { key },
            Effect::CheckActivity { key, id } => ToBackend::CheckActivity { key, id },
            // A folder that is gone (a removed worktree, say) would start nothing and learn
            // nothing: the look runs at home instead.
            Effect::Look { folder } if !folder.is_dir() => ToBackend::Look {
                folder: ui.app.home().to_path_buf(),
            },
            Effect::Look { folder } => ToBackend::Look { folder },
            Effect::ReadBranch { .. } if !ui.reads_branches => continue,
            Effect::ReadBranch { folder } => ToBackend::ReadBranch { folder },
            Effect::Prepare { chip, source } => ToBackend::Prepare { chip, source },
            Effect::ReadImage {
                picture,
                id,
                message,
                index,
            } => ToBackend::ReadImage {
                picture,
                id,
                message,
                index,
            },
            Effect::RemoveStored {
                id,
                unless_after_unix,
            } => ToBackend::RemoveStored {
                id,
                unless_after_unix,
            },
            Effect::Notify { key, title, alert } => {
                tell_notifier(ui, Note::Show { key, title, alert });
                continue;
            }
            Effect::Unnotify { key } => {
                tell_notifier(ui, Note::Close { key });
                continue;
            }
            Effect::UnnotifyAll => {
                tell_notifier(ui, Note::CloseAll);
                continue;
            }
        };
        match &ui.backend {
            Some(backend) => backend.send(message),
            None => ui.waiting.push(message),
        }
    }
    tidy_views(ui);
    tidy_sidebar(ui);
    tidy_handles(ui);
    if ui
        .file_drop
        .is_some_and(|key| !ui.app.tiles.contains(key) || !ui.app.takes_message(key))
    {
        ui.file_drop = None;
    }
    // The tile the user types in is the focused one. When the keyboard was in that tile
    // already, an intent moved the focus away and it is handed back: that is not the user's
    // act, and a ready session there stays ready. When the keyboard has come into the tile
    // since the last run (a click into its box, which Rune carries out without the app's
    // code; `poll` runs this before the view's own watch could), it is their act on that
    // session.
    let typing = typing_in(ui);
    if let Some(typing) = typing
        && ui.app.focused() != Some(typing)
    {
        let effects = if ui.typed_in == Some(typing) {
            ui.app.return_focus(typing)
        } else {
            ui.app.focus_tile(typing)
        };
        ui.typed_in = Some(typing);
        run(ui, effects);
    }
    ui.typed_in = typing;
}

/// What a held divider or line keeps for a cancel is forgotten when the tiles' placement
/// changed since the last [`run`]: a tile came, went or moved, or the focus did. Put back, it
/// could land on another pair of tiles. The drag itself goes on.
fn tidy_handles(ui: &mut Ui) {
    let placement = ui.app.tiles.placement();
    if placement != ui.was_placed {
        ui.was_placed = placement;
        ui.divider_press = None;
        ui.split_press = None;
    }
}

/// The sidebar's menu shuts when what it is the menu of is gone (a row's or a group's of the
/// list, also into or out of the Deleted view), and so does the folder menu of a group no longer
/// listed. A question about deleting goes when what it asks about is gone or deleted already, or
/// for "Delete All" when nothing is left in the Deleted view. A row's drag ends when its session
/// is gone or deleted or the Deleted view shows, a header's when its tile is gone, and either
/// while a menu is open: a menu takes the pointer, and the drag's release never comes.
///
/// A rename whose field is gone with its place ends: that of a group no longer listed, of a
/// session that is gone, of a row no longer listed, of a header whose session left its tile.
/// It ends dropped while the field still had the keyboard focus (the tile closed under it, say);
/// a session's name is kept when the field had already lost it (a click on another row shows
/// that session in the header's tile), as `view::sidebar::rename_watch` would have kept it.
fn tidy_sidebar(ui: &mut Ui) {
    let model = &ui.app.sidebar;
    let listed = |id: &GroupId| model.sections.iter().any(|section| section.id == *id);
    let sessions = model.view == SidebarView::Sessions;
    let deleted = |key: &SessionKey| {
        ui.app
            .sessions
            .get(*key)
            .map(|entry| entry.deleted.is_some())
    };
    let gone = match ui.side_menu.as_ref().map(|menu| &menu.target) {
        Some(MenuTarget::Row(key)) => deleted(key) != Some(false),
        Some(MenuTarget::Group(id)) => !sessions || !listed(id),
        Some(MenuTarget::HiddenGroups) => model.hidden.is_empty(),
        Some(MenuTarget::Deleted(key)) => deleted(key) != Some(true),
        Some(MenuTarget::DeletedGroup(id)) => sessions || !listed(id),
        None => false,
    };
    if gone {
        ui.side_menu = None;
    }
    let gone = match &ui.delete_ask {
        Some(DeleteAsk::Session(key)) => deleted(key) != Some(false),
        Some(DeleteAsk::Group(id)) => !sessions || !listed(id),
        Some(DeleteAsk::All) => sessions || model.sections.is_empty(),
        None => false,
    };
    if gone {
        ui.delete_ask = None;
    }
    let menu = ui.side_menu.is_some() || ui.folder_menu.is_some();
    if let Some(drag) = &ui.drag {
        let gone = match drag.from {
            DragFrom::Row => !sessions || deleted(&drag.key) != Some(false),
            DragFrom::Header => !ui.app.tiles.contains(drag.key),
        };
        if menu || gone {
            ui.drag = None;
        }
    }
    if let Some(Some(id)) = &ui.folder_menu
        && !listed(id)
    {
        ui.folder_menu = None;
        ui.folder_side = false;
    }
    let row_listed = |key: &SessionKey| {
        model
            .lines
            .iter()
            .any(|line| matches!(*line, Line::Row(s, r) if model.sections[s].rows[r].key == *key))
    };
    let (gone, session) = match &ui.renaming {
        Some(Renaming::Group(id)) => (!listed(id), false),
        Some(Renaming::Row(key)) => (!row_listed(key), true),
        Some(Renaming::Header(key)) => (!ui.app.tiles.contains(*key), true),
        None => (false, false),
    };
    if gone {
        let keep = session && !ui.rename.focused && ui.side_menu.is_none();
        end_rename(ui, keep);
    }
}

/// Brings every session's view up to what the core now holds:
///
/// - The view of a session that is gone is dropped.
/// - A session that was in a tile and is in none now gives up the keyboard focus and shuts its
///   picker list. One that came into a tile the core filled by itself (the first at the start, a
///   new session) is shown for the first time, at its end, with its input box focused when its
///   tile is the focused one. [`show`] makes the view first, and focuses the box only when asked
///   to. A tile that only gained or lost the focus keeps its picker and its fields as they are.
/// - A view whose `generation` is behind its session's forgets its rows opened, closed or shown
///   whole, and its open picker: the items were numbered again.
/// - The draft of a session that has ended gives up the keyboard focus: its box is drawn
///   disabled, with no field for the draft.
/// - The drafts of a request no longer waiting in its own session are dropped.
/// - A picker list open while its picker cannot be used is shut.
/// - The drawn picture of an id that no session needs any more (`Session::picture_ids`: its
///   chips', its queued message's, its conversation's) is dropped, and so is one for an id no
///   session ever held.
/// - The picture open large closes when its session is gone, deleted or no longer holds its id.
fn tidy_views(ui: &mut Ui) {
    let shown = ui.viewing.as_ref().is_none_or(|viewing| {
        ui.app.sessions.get(viewing.key).is_some_and(|entry| {
            entry.deleted.is_none() && entry.session.picture_ids().any(|id| id == viewing.picture)
        })
    });
    if !shown {
        close_picture(ui);
    }
    let Ui {
        app,
        views,
        was_tiled,
        thumbs,
        ..
    } = ui;
    views.retain(|key, _| app.sessions.get(*key).is_some());
    if !thumbs.is_empty() {
        let held: HashSet<u64> = app
            .sessions
            .iter()
            .flat_map(|entry| entry.session.picture_ids())
            .collect();
        thumbs.retain(|id, _| held.contains(id));
    }
    let tiled: Vec<SessionKey> = app.tiles.keys();
    if tiled != *was_tiled {
        for left in was_tiled.iter().filter(|key| !tiled.contains(key)) {
            if let Some(view) = views.get_mut(left) {
                view.leave();
            }
        }
        for &came in tiled.iter().filter(|key| !was_tiled.contains(key)) {
            let Some(entry) = app.sessions.get(came) else {
                continue;
            };
            views.entry(came).or_insert_with(|| {
                let mut view = SessionView::new(entry.session.generation);
                view.draft.focused = app.focused() == Some(came) && !ended(&entry.session);
                view
            });
        }
        *was_tiled = tiled;
    }
    for (key, view) in views.iter_mut() {
        let Some(entry) = app.sessions.get(*key) else {
            continue;
        };
        let session = &entry.session;
        if view.generation < session.generation {
            view.toggled.clear();
            view.whole.clear();
            view.open_picker = None;
            view.first_seen = None;
            view.generation = session.generation;
        }
        // Its box is drawn disabled from now on, with no field for its draft.
        if ended(session) {
            let_go(&mut view.draft);
        }
        drop_settled_drafts(view, session);
        shut_disabled_picker(view, session);
    }
}

/// Shuts the picker list that is open while its picker cannot be used (the session is opening or
/// has ended; for the model picker, also when there are no models to choose from; for the
/// effort picker, also when the model in use takes no level, and the picker is not shown), so it
/// does not open again by itself once it can.
fn shut_disabled_picker(view: &mut SessionView, session: &Session) {
    let usable = !matches!(session.phase, Phase::Opening | Phase::Ended { .. });
    let gone = match view.open_picker {
        Some(Picker::Model) => session.models.is_empty(),
        Some(Picker::Effort) => session.effort_levels().is_empty(),
        Some(Picker::Mode) | None => false,
    };
    if !usable || gone {
        view.open_picker = None;
    }
}

/// How long quitting waits for the sessions to end, all of them together: a little more than
/// the adapter gives `claude` after its input closes before it kills it, so their end is always
/// seen.
const CLOSE_WAIT: Duration = Duration::from_millis(2500);

/// `App::on_close` where the app keeps running without its window: the window goes, the app
/// and its sessions stay. The state is saved, what lies over the window is put away, and the
/// core is told that no window shows. Nothing while the app quits: [`quitting`] has ended it.
pub fn closing(ui: &mut Ui, _tx: &mut Tx) {
    if rune::quitting() {
        return;
    }
    let effects = ui.app.save_now();
    run(ui, effects);
    put_away(ui);
    let effects = ui.app.set_window(false);
    run(ui, effects);
}

/// `App::on_quit`, and `App::on_close` where closing the window ends the app: takes every
/// notification back, then ends every session, waiting up to [`CLOSE_WAIT`] for them, so no
/// program that runs one is left going without the app. Only the first call does anything.
///
/// The notifications go first, so that the notifier's thread has the time of that wait to
/// reach the desktop: a notification left behind by an app that is gone can only be dismissed,
/// and its press does nothing.
pub fn quitting(ui: &mut Ui, _tx: &mut Tx) {
    if ui.quitting {
        return;
    }
    ui.quitting = true;
    tell_notifier(ui, Note::CloseAll);
    if let Some(warning) = close_warning(ui, CLOSE_WAIT) {
        eprintln!("{warning}");
    }
}

/// Whether a quit asked for now shows its question instead of quitting: the question shows
/// already, or something would be cut off. What [`ask_quit`] then returns.
pub fn quit_asks(ui: &Ui) -> bool {
    ui.quit_ask || ui.app.busy() != Busy::default()
}

/// Quits, or asks first when something would be cut off. True when the question now shows.
pub fn ask_quit(ui: &mut Ui, tx: &mut Tx) -> bool {
    if !quit_asks(ui) {
        quit_now(ui, tx);
        return false;
    }
    if !ui.quit_ask {
        put_away(ui);
        ui.quit_ask = true;
    }
    true
}

/// Deletes `key`'s session at once when nothing is lost by it; else asks first
/// ([`delete_asks`]). Nothing for a session that is deleted or gone.
pub fn ask_delete_session(ui: &mut Ui, tx: &mut Tx, key: SessionKey) {
    match delete_asks(ui, key) {
        Some(true) => ask_delete(ui, DeleteAsk::Session(key)),
        Some(false) => delete(ui, tx, DeleteAsk::Session(key)),
        None => {}
    }
}

/// Whether deleting `key`'s session asks first; none for a session that is deleted or gone.
/// The core says what deleting would do (`App::deleting`): it asks when a program would be
/// stopped at work or a message not sent yet would be dropped. A session the store has nothing
/// of is forgotten for good, so it asks when anything is in it: the core knows its chips and
/// its message, the window the draft typed in its box.
pub fn delete_asks(ui: &Ui, key: SessionKey) -> Option<bool> {
    Some(match ui.app.deleting(key)? {
        Deleting::Kept => false,
        Deleting::Stops | Deleting::DropsMessage => true,
        Deleting::Forgotten { holds } => holds || !view_of(ui, key).draft.text.is_empty(),
    })
}

/// Asks before deleting the group `id` and every session of it. Not before the full list of
/// sessions is in: the question's count would not be the whole truth (`App::delete_group`).
pub fn ask_delete_group(ui: &mut Ui, id: GroupId) {
    if !ui.app.sidebar.complete {
        return;
    }
    ask_delete(ui, DeleteAsk::Group(id));
}

/// Asks before removing everything in the Deleted view for good.
pub fn ask_delete_all(ui: &mut Ui) {
    ask_delete(ui, DeleteAsk::All);
}

/// Shows the question about `ask`, after putting away what lies over the window: the question
/// whether to quit among it.
fn ask_delete(ui: &mut Ui, ask: DeleteAsk) {
    put_away(ui);
    ui.delete_ask = Some(ask);
}

/// Does what `what` asks and puts its question away. A deleted session's tile closes in the
/// core, and the tiles take their new widths at once; when it was the focused one, the input box
/// of the tile focused now takes the keyboard focus. Both as [`close_tile`] does. A delete that
/// closes no tile ("Delete All", a session in no tile) is not at once: the question and the
/// rows that leave go with their fade.
pub fn delete(ui: &mut Ui, tx: &mut Tx, what: DeleteAsk) {
    ui.delete_ask = None;
    let focused = ui.app.focused();
    let tiles = ui.app.tiles.keys().len();
    let effects = match &what {
        DeleteAsk::Session(key) => ui.app.delete_session(*key),
        DeleteAsk::Group(id) => ui.app.delete_group(id),
        DeleteAsk::All => ui.app.delete_all(),
    };
    if ui.app.tiles.keys().len() != tiles {
        tx.instant();
    }
    run(ui, effects);
    if focused.is_some_and(|key| !ui.app.tiles.contains(key))
        && let Some(now) = ui.app.focused()
    {
        type_in(ui, now);
    }
}

/// Ends the programs (`quitting`) and the app (`Tx::quit`).
/// They are told here, whether or not Rune runs `on_quit` for a window that is closed.
pub fn quit_now(ui: &mut Ui, tx: &mut Tx) {
    ui.quit_ask = false;
    quitting(ui, tx);
    tx.quit();
}

/// The app was launched again: with a folder that exists, a new session opens there, in the
/// focused tile, as a folder chosen in the folder dialog does. Only on the main screen, and
/// not while the app quits: nothing would end its program.
pub fn launched_again(ui: &mut Ui, tx: &mut Tx, folder: Option<PathBuf>) {
    let Some(folder) = folder else {
        return;
    };
    if ui.quitting || ui.app.screen != Screen::Main || !(ui.folder_exists)(&folder) {
        return;
    }
    view::folder::start(ui, tx, folder, None, Place::Focused);
}

/// `App::on_visibility`: the window went off screen or came back.
pub fn shown(ui: &mut Ui, shown: bool, _tx: &mut Tx) {
    let effects = ui.app.set_window(shown);
    run(ui, effects);
}

/// `App::on_focus`: the window got the keyboard or lost it to another app.
pub fn focus_changed(ui: &mut Ui, focused: bool, _tx: &mut Tx) {
    let effects = ui.app.set_active(focused);
    run(ui, effects);
}

/// Shuts what lies over the window: menus, pickers, a rename, a row's or a header's drag, the
/// question whether to quit or about deleting. A draft and its chips stay.
///
/// A divider, a line or a width being dragged is left alone here. As the window goes, Rune
/// cancels the pointer's drag: the handler hears its release as cancelled and puts back what it
/// kept at the press (`divider_press`, `split_press`), so that must still be there. That comes
/// after [`closing`] has saved, so the saved state holds the dragged shares until the next
/// save writes the ones put back.
pub fn put_away(ui: &mut Ui) {
    ui.quit_ask = false;
    ui.delete_ask = None;
    ui.side_menu = None;
    view::folder::close(ui);
    for view in ui.views.values_mut() {
        view.open_picker = None;
    }
    end_rename(ui, false);
    ui.drag = None;
    ui.file_drop = None;
    close_picture(ui);
}

/// Opens picture `picture` of `key`'s session large over the window, after focusing its tile
/// as a press there does and putting away what lies over the window. Its full image is loaded
/// at once when its bytes are at hand, else read from the store ([`take`] loads the answer);
/// until it shows, the small picture stands in. With neither, it says the full picture cannot
/// be read.
pub fn open_picture(ui: &mut Ui, key: SessionKey, picture: u64) {
    focus_tile(ui, key, false);
    put_away(ui);
    ui.zoom.reset();
    let (bytes, effects) = ui.app.full_image(key, picture);
    let full = bytes.and_then(|bytes| Some(ui.images.as_ref()?.load(bytes.shared())));
    let failed = full.is_none() && effects.is_empty();
    ui.viewing = Some(Viewing {
        picture,
        key,
        full,
        failed,
        made: false,
    });
    run(ui, effects);
}

/// Closes the picture open large, which lets its full image go, and forgets its zoom.
pub fn close_picture(ui: &mut Ui) {
    ui.viewing = None;
    ui.zoom.reset();
}

/// Ends the sessions as [`close_session`] does, and says so when one had not ended after `limit`.
/// The demo's fake is only told to close, never waited for: nothing is wrong there, so there is
/// nothing to say.
pub(crate) fn close_warning(ui: &mut Ui, limit: Duration) -> Option<String> {
    if close_session(ui, limit) || ui.fake.is_some() {
        return None;
    }
    Some(format!(
        "mindrune: the session had not ended {} ms as the app quit",
        limit.as_millis()
    ))
}

/// Writes the state kept between runs, then sends `Close` for every session whose program has
/// not ended and waits up to `limit` in all for them to end: also for one that was sent its
/// `Close` before, as a program being stopped for idleness. True when no session is left
/// running. The demo's fake is only told to close: its time does not pass unless it is pumped,
/// so nothing would come.
///
/// This wait is the one exception to the rule that the window's thread never blocks. It is
/// bounded, and it happens only as the app quits: once it has, nothing would be left to see
/// that the program stops, and the adapter's own kill after its grace period runs on a thread
/// that dies with the app.
pub(crate) fn close_session(ui: &mut Ui, limit: Duration) -> bool {
    let effects = ui.app.close((ui.unix_clock)());
    // The state is written here, before the wait, so it is kept even if the wait is cut short.
    run(ui, effects);
    if ui.app.all_ended() {
        return true;
    }
    if ui.fake.is_some() {
        return false;
    }
    let Some(backend) = &ui.backend else {
        return false;
    };
    let deadline = Instant::now() + limit;
    while !ui.app.all_ended() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let Some(message) = backend.recv_timeout(left) else {
            break;
        };
        // What a session would do next is not carried out: it is closing.
        let _ = ui.app.receive(message, 0.0);
    }
    ui.app.all_ended()
}

/// Drops the drafts of every request that no longer waits in `session`, whose view `view` is:
/// answered, withdrawn, or gone with a program that ended. Drafts of a request still waiting are
/// kept, whether the session is shown or not.
///
/// Only the end of the conversation is read: the walk stops once it has met every request
/// still waiting, which come after everything a finished turn left.
fn drop_settled_drafts(view: &mut SessionView, session: &Session) {
    if view.answers.is_empty() {
        return;
    }
    let mut waiting = Vec::new();
    let count = session.conversation.waiting();
    for item in session.conversation.items().iter().rev() {
        if waiting.len() == count {
            break;
        }
        if let ItemBody::Request {
            request,
            state: RequestState::Waiting,
        } = &item.body
        {
            waiting.push(&request.id);
        }
    }
    view.answers.retain(|id, _| waiting.contains(&id));
}

/// Takes every message waiting from the backend in one go, stamping events with `now`, and
/// carries out the effects that come back. True when at least one message was taken in.
pub(crate) fn take_in(ui: &mut Ui, now: f64) -> bool {
    let Some(backend) = &ui.backend else {
        return false;
    };
    let messages: Vec<FromBackend> = std::iter::from_fn(|| backend.try_recv()).collect();
    let took = !messages.is_empty();
    for message in messages {
        take(ui, message, now);
    }
    took
}

/// Takes one message from the backend in, stamped with `now`, and carries out its effects.
fn take(ui: &mut Ui, message: FromBackend, now: f64) {
    thumb(ui, &message);
    if let FromBackend::Prepared {
        chip,
        result: Err(_),
    } = &message
    {
        ui.reveal_chips.push(*chip);
    }
    if let FromBackend::Image { picture, result } = message {
        full_image(ui, picture, result);
        return;
    }
    let effects = ui.app.receive(message, now);
    run(ui, effects);
}

/// The full image of picture `picture`, read again: loaded when that picture is open large,
/// else dropped. One that could not be read says so. A picture opened, closed and opened
/// again before its first read answered gets two answers: once a good full image is there,
/// another answer changes nothing, whatever it says.
fn full_image(ui: &mut Ui, picture: u64, result: Result<ImageData, AgentError>) {
    let Some(viewing) = ui.viewing.as_mut().filter(|v| v.picture == picture) else {
        return;
    };
    if viewing.full.as_ref().is_some_and(|f| f.error().is_none()) {
        return;
    }
    match result {
        Ok(bytes) => {
            viewing.full = ui.images.as_ref().map(|images| images.load(bytes.shared()));
            viewing.failed = viewing.full.is_none();
        }
        Err(_) => viewing.failed = true,
    }
}

/// Whether the open picture's full image was decoded since the last call: the view draws it
/// in place of the small picture then, or, when it cannot be decoded, says so.
fn full_made(ui: &mut Ui) -> bool {
    let Some(viewing) = &mut ui.viewing else {
        return false;
    };
    let Some(full) = &viewing.full else {
        return false;
    };
    if viewing.made || !full.is_ready() {
        return false;
    }
    viewing.made = true;
    if full.error().is_some() {
        viewing.failed = true;
    }
    true
}

/// Whether a handle in `thumbs` was made since the last call: the rows that draw it are built
/// again (`view::items::version` reads its size). The ids whose handle is gone are forgotten.
fn handles_made(ui: &mut Ui) -> bool {
    let Ui {
        unready, thumbs, ..
    } = ui;
    let mut made = false;
    unready.retain(|id| match thumbs.get(id) {
        Some(image) if image.is_ready() => {
            made = true;
            false
        }
        Some(_) => true,
        None => false,
    });
    made
}

/// Makes the drawn picture that `message` brings, when it brings one and there is a cache to
/// make it in: a chip's thumbnail, kept by the chip's id, or a stored message's picture, kept
/// by its own. A view cannot make one: it only reads `Ui`. One for an id no session holds goes
/// in the same [`run`] ([`tidy_views`]).
fn thumb(ui: &mut Ui, message: &FromBackend) {
    let (id, thumb) = match message {
        FromBackend::Prepared {
            chip,
            result: Ok(Prepared {
                thumb: Some(thumb), ..
            }),
        } => (*chip, thumb),
        FromBackend::Picture {
            id,
            thumb: Some(thumb),
        } => (*id, thumb),
        _ => return,
    };
    if let Some(images) = &ui.images {
        let image = images.rgba(thumb.width, thumb.height, thumb.rgba.clone());
        ui.thumbs.insert(id, image);
        ui.unready.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo, testkit};
    use mr_agent::{
        Account, AgentError, Capabilities, EventSink, FakeAgent, FakeStep, HistoryItem,
        PermissionMode, SessionCommand, SessionEvent, SessionHandle, SessionId, SessionInfo,
        StartSpec, StoredHistory, StoredSession, ToolCallId, ToolDetail, ToolOutcome, TurnSummary,
    };
    use mr_core::{Alert, HistoryState, ItemBody, Phase, Screen, ThinkingTime, Thumb, Width};
    use rune::prefs::Prefs;
    use rune::{Key, Mods, Runtime};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_demo_window_reaches_the_main_screen_and_asks_for_usage() {
        let rt = testkit::window("empty");
        assert_eq!(rt.state.app.screen, Screen::Main);
        assert_eq!(rt.state.focused_session().unwrap().phase, Phase::Idle);
        assert!(testkit::sent(&rt).contains(&SessionCommand::RefreshUsage));
        let key = testkit::focused(&rt).unwrap();
        assert!(rt.find(&view::tile_key(key)).is_some());
        assert_eq!(rt.state.app.usage.session.as_ref().unwrap().percent(), 37);
    }

    #[test]
    fn each_account_state_has_its_screen() {
        for (scene, key) in [
            ("checking", "mr-screen-checking"),
            ("signed-out", "mr-screen-signin"),
            ("not-installed", "mr-screen-missing"),
            ("empty", view::ROW),
        ] {
            let rt = testkit::window(scene);
            assert!(rt.find(key).is_some(), "{scene}");
            assert!(rt.find("mr-root").is_some(), "{scene}");
        }
    }

    #[test]
    fn nothing_arriving_means_nothing_to_redraw() {
        let mut rt = testkit::window("empty");
        assert!(!demo::pump(&mut rt.state, 0.0));
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
        assert_eq!(
            rt.state.focused_session().unwrap().title.as_deref(),
            Some("Fix the parser")
        );
    }

    #[test]
    fn in_the_demo_events_are_stamped_with_the_fakes_time() {
        let mut ui = demo::ui();
        (demo::SCENES
            .iter()
            .find(|(name, _)| *name == "empty")
            .unwrap()
            .1)(&mut ui);
        let fake = ui.fake.clone().unwrap();
        let id = ui.focused_session().unwrap().id.clone().unwrap();
        fake.emit(&id, SessionEvent::ThinkingStarted);
        demo::pump(&mut ui, 0.0);
        fake.emit(&id, SessionEvent::BlockFinished);
        demo::pump(&mut ui, 4.0);
        let items = ui.focused_session().unwrap().conversation.items();
        let Some(ItemBody::Thinking { time, .. }) = items.last().map(|i| &i.body) else {
            panic!("no thinking item: {items:?}");
        };
        assert_eq!(*time, ThinkingTime::Took(4.0));
    }

    #[test]
    fn close_session_tells_the_demo_to_close_without_waiting() {
        let mut rt = testkit::window("working");
        let started = Instant::now();
        assert!(!close_session(&mut rt.state, CLOSE_WAIT));
        assert!(started.elapsed() < Duration::from_millis(500), "no wait");
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(
            testkit::sent(&rt).last(),
            Some(&SessionCommand::Close),
            "{:?}",
            testkit::sent(&rt)
        );
        let mut rt = testkit::window("ended");
        let before = testkit::sent(&rt).len();
        assert!(close_session(&mut rt.state, CLOSE_WAIT), "nothing to close");
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(testkit::sent(&rt).len(), before);
    }

    #[test]
    fn close_warning_has_nothing_to_say_in_the_demo() {
        let mut rt = testkit::window("working");
        assert_eq!(close_warning(&mut rt.state, CLOSE_WAIT), None);
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(testkit::sent(&rt).last(), Some(&SessionCommand::Close));
    }

    /// An agent whose sessions start at once and end `exit_after` a `Close`, or never.
    struct SlowExit {
        exit_after: Option<Duration>,
        closes: Arc<AtomicUsize>,
    }

    struct SlowExitHandle {
        sink: Arc<Mutex<EventSink>>,
        exit_after: Option<Duration>,
        closes: Arc<AtomicUsize>,
    }

    impl Agent for SlowExit {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                remote_access: false,
                plan_mode: true,
                usage_limits: false,
                images: false,
            }
        }
        fn account(&self) -> Account {
            Account::SignedIn {
                name: "alex".into(),
                plan: None,
                full_login: true,
            }
        }
        fn list_sessions(&self, _: Option<i64>) -> Result<Vec<StoredSession>, AgentError> {
            Ok(Vec::new())
        }
        fn read_session(&self, _: &SessionId, _: usize) -> Result<StoredHistory, AgentError> {
            Ok(StoredHistory::default())
        }
        fn last_active(&self, _: &SessionId) -> Result<Option<i64>, AgentError> {
            Ok(None)
        }
        fn open(
            &self,
            spec: StartSpec,
            sink: EventSink,
        ) -> Result<Box<dyn SessionHandle>, AgentError> {
            sink(SessionEvent::Started(SessionInfo {
                id: SessionId("s1".into()),
                folder: spec.folder,
                model: "claude-haiku-5-5".into(),
                permission_mode: PermissionMode::Ask,
                agent_version: "2.1.319".into(),
                models: Vec::new(),
            }));
            Ok(Box::new(SlowExitHandle {
                sink: Arc::new(Mutex::new(sink)),
                exit_after: self.exit_after,
                closes: self.closes.clone(),
            }))
        }
    }

    impl SessionHandle for SlowExitHandle {
        fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
            if command == SessionCommand::Close {
                self.closes.fetch_add(1, Ordering::SeqCst);
                if let Some(after) = self.exit_after {
                    let sink = self.sink.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(after);
                        (sink.lock().unwrap())(SessionEvent::Exited {
                            clean: true,
                            detail: String::new(),
                        });
                    });
                }
            }
            Ok(())
        }
    }

    /// A window state over `agent` (not the demo's fake), its session started and idle.
    fn idle_over(agent: Arc<SlowExit>) -> Ui {
        let launch = Launch {
            folder: Some("/home/alex/project".into()),
            ..Launch::default()
        };
        let mut ui = Ui::new(agent, None, launch, Prefs::memory());
        setup(&mut ui, Arc::new(|| {}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ui.focused_session().is_some_and(|s| s.phase == Phase::Idle) {
            take_in(&mut ui, 0.0);
            assert!(Instant::now() < deadline, "the session never started");
            std::thread::sleep(Duration::from_millis(2));
        }
        ui
    }

    #[test]
    fn close_session_waits_for_the_session_to_end() {
        let agent = Arc::new(SlowExit {
            exit_after: Some(Duration::from_millis(300)),
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let started = Instant::now();
        assert!(close_session(&mut ui, CLOSE_WAIT));
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(
            waited < CLOSE_WAIT,
            "it stops waiting once the session ended: {waited:?}"
        );
        assert!(ui.app.all_ended());
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn close_warning_waits_no_longer_than_its_limit() {
        let agent = Arc::new(SlowExit {
            exit_after: None,
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let limit = Duration::from_millis(200);
        let started = Instant::now();
        assert_eq!(
            close_warning(&mut ui, limit).as_deref(),
            Some("mindrune: the session had not ended 200 ms as the app quit")
        );
        let waited = started.elapsed();
        assert!(
            waited >= limit && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn close_session_waits_for_a_program_being_stopped_for_idleness() {
        let agent = Arc::new(SlowExit {
            exit_after: Some(Duration::from_millis(300)),
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let key = ui.app.focused().unwrap();
        // A message and the end of its turn: the agent stores the session, and it idles.
        let effects = ui.app.submit(key, "hello");
        run(&mut ui, effects);
        let open = ui.app.sessions.get(key).unwrap().open.unwrap();
        let done = SessionEvent::TurnFinished(mr_agent::TurnSummary {
            cost_usd: 0.01,
            context_used: None,
            duration_ms: 1,
            interrupted: false,
        });
        ui.app.event(open, done, 0.0);
        // Half an hour later the tick stops it, and the quit comes while it goes.
        let effects = ui.app.tick((ui.unix_clock)() + 3600);
        let close = Effect::Send {
            open,
            command: SessionCommand::Close,
        };
        assert!(effects.contains(&close), "{effects:?}");
        run(&mut ui, effects);
        assert_eq!(ui.app.sessions.get(key).unwrap().stopping, Some(open));
        assert!(close_session(&mut ui, CLOSE_WAIT));
        assert!(
            ui.app.all_ended(),
            "the program being stopped was waited for"
        );
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1, "no second Close");
    }

    /// What the window says when the saved state cannot be read.
    const UNREADABLE: &str = "mindrune: the saved state could not be read; starting from nothing";

    /// A window state over the fake, not started, with `prefs`; and what it would say.
    fn opened(prefs: Prefs) -> (Ui, Option<String>) {
        let fake = FakeAgent::new();
        Ui::open(Arc::new(fake.clone()), Some(fake), Launch::default(), prefs)
    }

    #[test]
    fn a_state_that_cannot_be_read_starts_from_nothing() {
        let (fresh, warning) = opened(Prefs::memory());
        assert_eq!(warning, None, "nothing saved yet is nothing to say");
        let nothing = fresh.app.saved();
        assert_eq!(nothing.version, SAVED_VERSION);

        let mut prefs = Prefs::memory();
        prefs.set(STATE_KEY, "not a state").unwrap();
        let (ui, warning) = opened(prefs);
        assert_eq!(warning.as_deref(), Some(UNREADABLE));
        assert_eq!(ui.app.saved(), nothing);
        assert_eq!((ui.sidebar.width(), ui.sidebar.is_open()), (260.0, true));

        let mut prefs = Prefs::memory();
        let newer = Saved {
            version: SAVED_VERSION + 1,
            recent_folders: vec!["/home/alex/project".into()],
            sidebar_width: Some(380.0),
            sidebar_collapsed: true,
            ..Saved::default()
        };
        prefs.set(STATE_KEY, &newer).unwrap();
        let (ui, warning) = opened(prefs);
        assert_eq!(warning.as_deref(), Some(UNREADABLE), "another version");
        assert_eq!(ui.app.saved(), nothing);
        assert_eq!((ui.sidebar.width(), ui.sidebar.is_open()), (260.0, true));
    }

    #[test]
    fn a_state_that_cannot_be_read_is_kept_aside() {
        let (fresh, _) = opened(Prefs::memory());
        assert!(!fresh.prefs.contains(UNREAD_KEY), "nothing to keep aside");

        let mut prefs = Prefs::memory();
        prefs.set(STATE_KEY, "not a state").unwrap();
        let (ui, _) = opened(prefs);
        assert_eq!(
            ui.prefs.get::<String>(UNREAD_KEY).as_deref(),
            Some("not a state")
        );

        let mut prefs = Prefs::memory();
        let newer = Saved {
            version: SAVED_VERSION + 1,
            recent_folders: vec!["/home/alex/project".into()],
            ..Saved::default()
        };
        prefs.set(STATE_KEY, &newer).unwrap();
        let (mut ui, _) = opened(prefs);
        let effects = ui.app.new_group("Side projects").1;
        run(&mut ui, effects);
        let written: Saved = ui.prefs.get(STATE_KEY).unwrap();
        assert_eq!(
            written.version, SAVED_VERSION,
            "this build's state is written"
        );
        assert_eq!(
            ui.prefs.get::<Saved>(UNREAD_KEY),
            Some(newer),
            "the other version's is still there"
        );

        // A state this build reads is not copied.
        let mut prefs = std::mem::replace(&mut ui.prefs, Prefs::memory());
        prefs.remove(UNREAD_KEY).unwrap();
        let (back, warning) = opened(prefs);
        assert_eq!(warning, None);
        assert!(!back.prefs.contains(UNREAD_KEY));
    }

    #[test]
    fn a_look_in_a_folder_that_is_gone_runs_at_home() {
        let home = tempfile::tempdir().unwrap();
        let there = tempfile::tempdir().unwrap();
        let fake = FakeAgent::new();
        let launch = Launch {
            home: home.path().into(),
            ..Launch::default()
        };
        let (mut ui, _) = Ui::open(Arc::new(fake.clone()), Some(fake), launch, Prefs::memory());
        let gone = there.path().join("removed-worktree");
        run(
            &mut ui,
            vec![
                Effect::Look {
                    folder: there.path().into(),
                },
                Effect::Look { folder: gone },
            ],
        );
        assert_eq!(
            ui.waiting[ui.waiting.len() - 2..],
            [
                ToBackend::Look {
                    folder: there.path().into()
                },
                ToBackend::Look {
                    folder: home.path().into()
                },
            ]
        );
    }

    #[test]
    fn the_state_is_written_and_read_back() {
        // A message was handed over: the store has the session, so its tile is kept.
        let mut rt = testkit::window("conversation");
        let ui = &mut rt.state;
        let (_, effects) = ui.app.new_group("Side projects");
        run(ui, effects);
        ui.sidebar.set_width(320.0);
        ui.sidebar.set_open(false);
        // The window closes a while after its last poll: the clock is read as it closes.
        ui.unix_clock = || demo::NOW_UNIX + 25;
        close_session(ui, CLOSE_WAIT);
        let written: Saved = ui.prefs.get(STATE_KEY).expect("written on closing");
        let closed_at: Vec<i64> = written.closed_by_app.iter().map(|(_, at)| *at).collect();
        assert_eq!(closed_at, vec![demo::NOW_UNIX + 25]);
        assert_eq!(
            (written.sidebar_width, written.sidebar_collapsed),
            (Some(320.0), true)
        );
        assert_eq!(written.groups.len(), 1);
        let project = std::path::PathBuf::from("/home/alex/project");
        assert_eq!(written.recent_folders, vec![project], "the demo's folder");
        assert_eq!(written.tiles.len(), 1);
        assert_eq!(written.shown.as_ref(), Some(&written.tiles[0].id));

        let prefs = std::mem::replace(&mut ui.prefs, Prefs::memory());
        let (back, warning) = opened(prefs);
        assert_eq!(warning, None);
        assert_eq!(
            (back.sidebar.width(), back.sidebar.is_open()),
            (320.0, false)
        );
        // What this app closed is kept only while it is recent, by the wall clock, which is
        // not the demo's: it is left out of the comparison.
        assert_eq!(
            Saved {
                closed_by_app: Vec::new(),
                ..back.app.saved()
            },
            Saved {
                sidebar_width: None,
                sidebar_collapsed: false,
                panel_width: None,
                panel_open: false,
                closed_by_app: Vec::new(),
                ..written
            }
        );
    }

    #[test]
    fn the_panel_is_saved_and_comes_back() {
        let (fresh, _) = opened(Prefs::memory());
        assert_eq!((fresh.panel.width(), fresh.panel.is_open()), (300.0, false));
        let mut rt = testkit::window("conversation");
        let ui = &mut rt.state;
        ui.panel.set_width(360.0);
        ui.panel.set_open(true);
        let effects = ui.app.save_now();
        run(ui, effects);
        let written: Saved = ui.prefs.get(STATE_KEY).expect("written");
        assert_eq!(
            (written.panel_width, written.panel_open),
            (Some(360.0), true)
        );
        let prefs = std::mem::replace(&mut ui.prefs, Prefs::memory());
        let (back, _) = opened(prefs);
        assert_eq!((back.panel.width(), back.panel.is_open()), (360.0, true));
        assert_eq!(
            (back.sidebar.width(), back.sidebar.is_open()),
            (260.0, true),
            "the sidebar is its own"
        );

        // Hidden, it comes back hidden, at its width.
        let mut prefs = Prefs::memory();
        let hidden = Saved {
            version: SAVED_VERSION,
            panel_width: Some(420.0),
            panel_open: false,
            ..Saved::default()
        };
        prefs.set(STATE_KEY, &hidden).unwrap();
        let (back, _) = opened(prefs);
        assert_eq!((back.panel.width(), back.panel.is_open()), (420.0, false));
    }

    #[test]
    fn showing_the_panel_saves_at_once() {
        let mut rt = testkit::window("conversation");
        let written = |rt: &Runtime<Ui>| -> Option<(Option<f32>, bool)> {
            let saved: Saved = rt.state.prefs.get(STATE_KEY)?;
            Some((saved.panel_width, saved.panel_open))
        };
        rt.state.prefs.remove(STATE_KEY).unwrap();
        testkit::click(&mut rt, "mr-panel-toggle");
        assert_eq!(written(&rt), Some((Some(300.0), true)), "the button");
        rt.state.prefs.remove(STATE_KEY).unwrap();
        let ctrl_alt = Mods {
            alt: true,
            ..testkit::ctrl()
        };
        testkit::press_with(&mut rt, Key::Command('b'), ctrl_alt);
        assert_eq!(written(&rt), Some((Some(300.0), false)), "the key");
    }

    #[test]
    fn a_session_that_is_gone_leaves_no_view() {
        let mut rt = testkit::window("empty");
        let first = testkit::focused(&rt).unwrap();
        assert!(
            rt.state.views.contains_key(&first),
            "the shown session has its view"
        );
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        assert!(
            rt.state.app.sessions.get(first).is_none(),
            "the empty one was forgotten"
        );
        assert!(!rt.state.views.contains_key(&first));
        assert!(rt.state.views.contains_key(&a));
    }

    #[test]
    fn history_that_replaces_a_conversation_forgets_opened_rows() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        testkit::emit_to(
            &mut rt,
            a,
            SessionEvent::ToolStarted {
                id: ToolCallId("c1".into()),
                name: "Bash".into(),
                detail: ToolDetail::Command {
                    command: "ls".into(),
                },
            },
        );
        testkit::emit_to(
            &mut rt,
            a,
            SessionEvent::ToolFinished {
                id: ToolCallId("c1".into()),
                outcome: ToolOutcome::Ok {
                    output: "notes.txt\n".into(),
                },
            },
        );
        let id = testkit::shown_session_items(&rt).last().unwrap().id;
        testkit::click(&mut rt, &format!("mr-toggle-{}", id.0));
        view_mut(&mut rt.state, a).whole.insert(id);
        view_mut(&mut rt.state, a).open_picker = Some(Picker::Mode);
        assert!(view_of(&rt.state, a).toggled.contains(&id));
        // The stored history arrives late, after the program has added to the conversation.
        let ui = &mut rt.state;
        ui.app.sessions.get_mut(a).unwrap().session.history = HistoryState::Loading;
        let generation = ui.app.sessions.get(a).unwrap().session.generation;
        let history = StoredHistory {
            items: vec![HistoryItem::User {
                text: "Earlier".into(),
                image_count: 0,
                files: Vec::new(),
                images: Vec::new(),
                stored: None,
            }],
            ..StoredHistory::default()
        };
        let effects = ui.app.receive(
            FromBackend::History {
                key: a,
                result: Ok(history),
                pictures: Vec::new(),
            },
            0.0,
        );
        run(ui, effects);
        let session = &ui.app.sessions.get(a).unwrap().session;
        assert_eq!(session.generation, generation + 1);
        let view = view_of(ui, a);
        assert!(view.toggled.is_empty() && view.whole.is_empty());
        assert_eq!(view.open_picker, None);
        assert_eq!(view.generation, session.generation);
        rt.invalidate();
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Earlier"));
        assert!(testkit::has_text(&rt, "Ran ls"));
        assert!(
            !testkit::has_text(&rt, "notes.txt"),
            "the listing is closed"
        );
    }

    #[test]
    fn quitting_ends_every_session_and_saves() {
        let mut rt = testkit::window("empty");
        for folder in ["/home/alex/a", "/home/alex/b", "/home/alex/c"] {
            testkit::add_session(&mut rt, folder);
        }
        assert_eq!(rt.state.app.sessions.len(), 3);
        let fake = rt.state.fake.clone().unwrap();
        // The scene's own session, forgotten, was closed already.
        let before = fake.sent().len();
        rt.state.prefs.remove(STATE_KEY).unwrap();
        quitting(&mut rt.state, &mut Tx::default());
        rt.state.backend.as_ref().unwrap().flush();
        let closed: HashSet<SessionId> = fake.sent()[before..]
            .iter()
            .filter(|(_, command)| *command == SessionCommand::Close)
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(closed.len(), 3, "{:?}", fake.sent());
        assert!(rt.state.prefs.get::<Saved>(STATE_KEY).is_some());
    }

    #[test]
    fn quitting_ends_every_program_once() {
        let mut rt = testkit::window("empty");
        for folder in ["/home/alex/a", "/home/alex/b"] {
            testkit::add_session(&mut rt, folder);
        }
        let fake = rt.state.fake.clone().unwrap();
        let before = fake.sent().len();
        quitting(&mut rt.state, &mut Tx::default());
        quitting(&mut rt.state, &mut Tx::default());
        assert!(rt.state.quitting);
        rt.state.backend.as_ref().unwrap().flush();
        let mut closed: Vec<SessionId> = fake.sent()[before..]
            .iter()
            .filter(|(_, command)| *command == SessionCommand::Close)
            .map(|(id, _)| id.clone())
            .collect();
        closed.sort_by(|a, b| a.0.cmp(&b.0));
        let count = closed.len();
        closed.dedup();
        assert_eq!((count, closed.len()), (2, 2), "{:?}", fake.sent());
    }

    #[test]
    fn closing_the_window_ends_no_program() {
        let mut rt = testkit::window("empty");
        let key = testkit::add_session(&mut rt, "/home/alex/a");
        let fake = rt.state.fake.clone().unwrap();
        let before = fake.sent().len();
        rt.state.prefs.remove(STATE_KEY).unwrap();
        closing(&mut rt.state, &mut Tx::default());
        rt.state.backend.as_ref().unwrap().flush();
        assert!(
            !fake.sent()[before..]
                .iter()
                .any(|(_, command)| *command == SessionCommand::Close),
            "{:?}",
            fake.sent()
        );
        assert!(!rt.state.quitting);
        let session = &rt.state.app.sessions.get(key).unwrap().session;
        assert_eq!(session.phase, Phase::Idle);
        assert!(rt.state.prefs.get::<Saved>(STATE_KEY).is_some(), "saved");
    }

    #[test]
    fn a_turn_goes_on_with_the_window_closed() {
        let mut rt = testkit::window("empty");
        let key = testkit::add_session(&mut rt, "/home/alex/a");
        let fake = rt.state.fake.clone().unwrap();
        fake.reply(vec![
            FakeStep::at(0.0, SessionEvent::TurnStarted),
            FakeStep::at(0.5, SessionEvent::TextDelta("All tidy now.".into())),
            FakeStep::at(
                1.0,
                SessionEvent::TurnFinished(TurnSummary {
                    cost_usd: 0.0,
                    context_used: None,
                    duration_ms: 1,
                    interrupted: false,
                }),
            ),
        ]);
        let ui = &mut rt.state;
        let effects = ui.app.submit(key, "Tidy the notes");
        run(ui, effects);
        closing(ui, &mut Tx::default());
        let replied = |ui: &Ui| {
            ui.app.sessions.get(key).is_some_and(|entry| {
                entry.session.phase == Phase::Idle
                    && entry.session.conversation.items().iter().any(|item| {
                        matches!(&item.body, ItemBody::Reply { markdown, .. }
                            if markdown == "All tidy now.")
                    })
            })
        };
        let mut now = 100.0;
        while !replied(ui) && now < 110.0 {
            now += 0.25;
            poll(ui, now, &mut Tx::default());
        }
        assert!(replied(ui), "the reply came in with no window");
        let entry = ui.app.sessions.get(key).unwrap();
        assert!(entry.unseen, "no one saw it end");
    }

    #[test]
    fn losing_the_keyboard_is_told_to_the_core() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.state.fake.clone().unwrap().reply(vec![
            FakeStep::at(0.0, SessionEvent::TurnStarted),
            FakeStep::at(
                1.0,
                SessionEvent::TurnFinished(TurnSummary {
                    cost_usd: 0.0,
                    context_used: None,
                    duration_ms: 1,
                    interrupted: false,
                }),
            ),
        ]);
        let ui = &mut rt.state;
        assert!(ui.app.looking(), "active from the start");
        focus_changed(ui, false, &mut Tx::default());
        assert!(!ui.app.looking());
        let effects = ui.app.submit(key, "Tidy the notes");
        run(ui, effects);
        let mut now = 100.0;
        while ui.app.sessions.get(key).unwrap().session.phase != Phase::Idle && now < 110.0 {
            now += 0.25;
            poll(ui, now, &mut Tx::default());
        }
        assert_eq!(ui.app.focused(), Some(key));
        assert!(
            ui.app.sessions.get(key).unwrap().unseen,
            "ready in the focused tile"
        );
        focus_changed(ui, true, &mut Tx::default());
        assert!(ui.app.looking());
        assert!(
            ui.app.sessions.get(key).unwrap().unseen,
            "until the user acts on it"
        );
    }

    #[test]
    fn a_notify_effect_reaches_the_notifier_and_is_dropped_without_one() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let effects = || {
            vec![
                Effect::Notify {
                    key,
                    title: "Tidy the notes".into(),
                    alert: Alert::NeedsInput,
                },
                Effect::Unnotify { key },
                Effect::UnnotifyAll,
            ]
        };
        let sent = testkit::sent(&rt).len();
        // No notifier: they are dropped, and nothing goes to the programs.
        run(&mut rt.state, effects());
        assert_eq!(testkit::sent(&rt).len(), sent);
        let (notifier, told) = std::sync::mpsc::channel();
        rt.state.notifier = Some(notifier);
        run(&mut rt.state, effects());
        assert_eq!(
            told.try_iter().collect::<Vec<_>>(),
            vec![
                Note::Show {
                    key,
                    title: "Tidy the notes".into(),
                    alert: Alert::NeedsInput,
                },
                Note::Close { key },
                Note::CloseAll,
            ]
        );
        assert_eq!(testkit::sent(&rt).len(), sent);
    }

    #[test]
    fn a_pressed_alert_focuses_its_session() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        assert_eq!(testkit::focused(&rt), Some(a));
        rt.update(|ui, tx| alert_pressed(ui, tx, b));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(testkit::box_focused(&rt, b), "the keyboard is in its box");
        assert!(!testkit::box_focused(&rt, a));
    }

    #[test]
    fn quitting_takes_the_notifications_back_first() {
        let mut rt = testkit::window("working");
        let (notifier, told) = std::sync::mpsc::channel();
        rt.state.notifier = Some(notifier);
        let before = testkit::sent(&rt).len();
        let ui = &mut rt.state;
        quitting(ui, &mut Tx::default());
        assert_eq!(
            told.try_iter().collect::<Vec<_>>(),
            vec![Note::CloseAll],
            "whatever is out, and whether or not the user looks"
        );
        ui.backend.as_ref().unwrap().flush();
        assert_eq!(
            testkit::sent(&rt).len(),
            before + 1,
            "then the programs end"
        );
        // Only the first call does anything.
        quitting(&mut rt.state, &mut Tx::default());
        assert_eq!(told.try_iter().count(), 0);
    }

    #[test]
    fn the_tray_is_told_when_the_standing_changes_and_only_then() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        // The turn starts at once and ends only after the polls here.
        rt.state.fake.clone().unwrap().reply(vec![
            FakeStep::at(0.0, SessionEvent::TurnStarted),
            FakeStep::at(
                60.0,
                SessionEvent::TurnFinished(TurnSummary {
                    cost_usd: 0.0,
                    context_used: None,
                    duration_ms: 1,
                    interrupted: false,
                }),
            ),
        ]);
        let ui = &mut rt.state;
        // The standing changes before the tray is there: it is told once the tray is.
        ui.app.sessions.get_mut(key).unwrap().unseen = true;
        poll(ui, 0.0, &mut Tx::default());
        let (tray, told) = std::sync::mpsc::channel();
        ui.tray = Some(tray);
        poll(ui, 0.0, &mut Tx::default());
        poll(ui, 0.0, &mut Tx::default());
        let finished = Standing {
            finished: 1,
            ..Standing::default()
        };
        assert_eq!(told.try_iter().collect::<Vec<_>>(), vec![finished]);
        let effects = ui.app.submit(key, "Tidy the notes");
        run(ui, effects);
        poll(ui, 0.0, &mut Tx::default());
        poll(ui, 0.0, &mut Tx::default());
        let working = Standing {
            working: 1,
            ..Standing::default()
        };
        assert_eq!(told.try_iter().collect::<Vec<_>>(), vec![working]);
    }

    #[test]
    fn closing_the_window_still_delivers_a_message_its_program_never_got() {
        let mut rt = testkit::window("empty");
        let key = testkit::add_session(&mut rt, "/home/alex/a");
        let ui = &mut rt.state;
        closing(ui, &mut Tx::default());
        let open = ui.app.sessions.get(key).unwrap().open.unwrap();
        // The message goes as its program ends: the backend refuses it, then the exit comes.
        let _ = ui.app.submit(key, "One more thing");
        let refused = FromBackend::SendFailed {
            open,
            command: SessionCommand::Send {
                text: "One more thing".into(),
                attachments: Vec::new(),
            },
            error: AgentError::Closed,
        };
        assert!(ui.app.receive(refused, 0.0).is_empty());
        let exited = FromBackend::Event {
            open,
            event: SessionEvent::Exited {
                clean: true,
                detail: String::new(),
            },
        };
        let effects = ui.app.receive(exited, 0.0);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Open { .. })),
            "started again: {effects:?}"
        );
    }

    #[test]
    fn closing_puts_away_what_was_open() {
        let mut rt = testkit::window("empty");
        let key = testkit::add_session(&mut rt, "/home/alex/a");
        let ui = &mut rt.state;
        let title = ui.app.sessions.get(key).unwrap().session.title.clone();
        view_mut(ui, key).draft = TextEdit::new("Half a thought");
        view_mut(ui, key).open_picker = Some(Picker::Model);
        start_rename(ui, Renaming::Row(key));
        ui.rename = TextEdit::new("Not kept");
        ui.side_menu = Some(SideMenu {
            target: MenuTarget::Row(key),
            at: PointerPress { x: 0.0, y: 0.0 },
            sub_open: true,
        });
        ui.folder_menu = Some(None);
        ui.folder_offers = vec!["/home/alex/a".into(), "/home/alex/b".into()];
        ui.folder_side = true;
        ui.file_drop = Some(key);
        closing(ui, &mut Tx::default());
        assert_eq!(view_of(ui, key).open_picker, None);
        assert_eq!(view_of(ui, key).draft.text, "Half a thought");
        assert_eq!(ui.renaming, None);
        assert_eq!(ui.app.sessions.get(key).unwrap().session.title, title);
        assert_eq!(ui.side_menu, None);
        assert_eq!((ui.folder_menu.clone(), ui.folder_side), (None, false));
        assert!(ui.folder_offers.is_empty(), "the menu's folders go with it");
        assert_eq!(ui.file_drop, None);
        // A drag on its own: an open menu would have ended it already.
        ui.drag = Some(RowDrag {
            key,
            at: [10.0, 10.0],
            over: None,
            heard: true,
            missed: 0,
            from: DragFrom::Row,
        });
        closing(ui, &mut Tx::default());
        assert_eq!(ui.drag, None);
    }

    #[test]
    fn the_width_is_read_from_settings_and_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut prefs = Prefs::at(&path);
        prefs
            .set("conversation_width", &Width::Points(720.0))
            .unwrap();
        let fake = FakeAgent::new();
        let launch = Launch {
            folder: Some("/p".into()),
            ..Launch::default()
        };
        let mut ui = Ui::new(Arc::new(fake.clone()), Some(fake), launch, Prefs::at(&path));
        assert_eq!(ui.app.width, Width::Points(720.0));
        let effects = ui.app.set_width(Width::Unlimited);
        run(&mut ui, effects);
        assert_eq!(
            Prefs::at(&path).get::<Width>("conversation_width"),
            Some(Width::Unlimited)
        );
    }

    /// A stored session whose history is `items`, listed and not shown.
    fn stored_with(rt: &mut Runtime<Ui>, id: &str, items: Vec<HistoryItem>) -> SessionKey {
        let id = SessionId(id.into());
        let history = StoredHistory {
            items,
            ..StoredHistory::default()
        };
        rt.state.fake.as_ref().unwrap().set_history(&id, history);
        let stored = StoredSession {
            id,
            folder: "/home/alex/notes".into(),
            title: "Notes".into(),
            last_active_unix: demo::NOW_UNIX - demo::DAY,
        };
        let key = rt
            .state
            .app
            .sessions
            .add(mr_core::Session::resting(&stored), stored.last_active_unix);
        run(&mut rt.state, Vec::new());
        key
    }

    /// A message of the user's with one stored image of `width` by `height` pixels, and
    /// Claude's answer.
    fn with_image(text: &str, width: u32, height: u32) -> Vec<HistoryItem> {
        let png = mr_agent::ImageData::new(testkit::png(width, height));
        vec![
            HistoryItem::User {
                text: text.into(),
                image_count: 1,
                files: Vec::new(),
                images: vec![Some(png)],
                stored: None,
            },
            HistoryItem::AssistantText("Seen.".into()),
        ]
    }

    /// The picture ids of `key`'s conversation.
    fn pictures(rt: &Runtime<Ui>, key: SessionKey) -> Vec<u64> {
        let session = &rt.state.app.sessions.get(key).unwrap().session;
        session.conversation.pictures().collect()
    }

    /// Waits for the backend's threads, so the frames after take in every answer they gave.
    fn settle_backend(rt: &mut Runtime<Ui>) {
        rt.state.backend.as_ref().expect("the backend").flush();
        testkit::step(rt);
    }

    /// Waits for every picture of `key`'s conversation to come and its handle to be made, and
    /// runs frames.
    fn pictures_come(rt: &mut Runtime<Ui>, key: SessionKey) {
        settle_backend(rt);
        for id in pictures(rt, key) {
            let handle = rt.state.thumbs.get(&id).expect("its handle").clone();
            handle.wait().expect("a picture made in the test is made");
        }
        testkit::step(rt);
    }

    fn cache() -> Option<ImageCache> {
        Some(ImageCache::new(Arc::new(|| {})))
    }

    #[test]
    fn a_stored_pictures_answer_makes_its_handle() {
        let mut rt = testkit::window("resting");
        rt.state.images = cache();
        let key = stored_with(&mut rt, "stored-wide", with_image("Look", 6, 3));
        testkit::show(&mut rt, key);
        let ids = pictures(&rt, key);
        assert_eq!(ids.len(), 1);
        assert_ne!(ids[0] & 1 << 63, 0, "a stored picture's id");
        pictures_come(&mut rt, key);
        assert_eq!(rt.state.thumbs[&ids[0]].size(), Some([6, 3]));
        let shown = format!("mr-user-picture-{}", ids[0]);
        let shown = testkit::element(&rt, &testkit::tile(key, &shown)).expect("drawn");
        assert_eq!(&shown.rect[2..], &[240.0, 120.0]);
    }

    #[test]
    fn a_handle_made_after_it_came_draws_once_more() {
        let mut rt = testkit::window("resting");
        rt.state.images = cache();
        let key = stored_with(&mut rt, "stored-late", with_image("Look", 6, 3));
        testkit::show(&mut rt, key);
        pictures_come(&mut rt, key);
        let id = pictures(&rt, key)[0];
        // The same picture's answer again: its new handle is made on the cache's threads.
        let thumb = Thumb {
            width: 2,
            height: 1,
            rgba: vec![90u8; 8].into(),
        };
        let mut redraw = true;
        rt.update(|ui, tx| redraw = poll(ui, 1.0, tx));
        assert!(!redraw, "nothing new");
        take(
            &mut rt.state,
            FromBackend::Picture {
                id,
                thumb: Some(thumb),
            },
            0.0,
        );
        rt.state.thumbs[&id].wait().expect("made");
        rt.update(|ui, tx| redraw = poll(ui, 1.0, tx));
        assert!(redraw, "a handle made since the last poll draws again");
        rt.update(|ui, tx| redraw = poll(ui, 1.0, tx));
        assert!(!redraw, "once");
    }

    #[test]
    fn a_picture_for_nobody_is_not_kept() {
        let mut rt = testkit::window("empty");
        rt.state.images = cache();
        let thumb = Thumb {
            width: 2,
            height: 1,
            rgba: vec![255u8; 8].into(),
        };
        let nobody = 1 << 63 | 4_242;
        take(
            &mut rt.state,
            FromBackend::Picture {
                id: nobody,
                thumb: Some(thumb),
            },
            0.0,
        );
        assert!(rt.state.thumbs.is_empty());
        let never = FromBackend::Picture {
            id: nobody + 1,
            thumb: None,
        };
        take(&mut rt.state, never, 0.0);
        assert!(rt.state.thumbs.is_empty());
    }

    #[test]
    fn a_sent_images_handle_outlives_its_chip() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.state.images = cache();
        testkit::paste_image(&mut rt, testkit::tiny_png());
        let chips = |rt: &Runtime<Ui>| {
            rt.state
                .app
                .sessions
                .get(key)
                .unwrap()
                .session
                .chips
                .clone()
        };
        settle_backend(&mut rt);
        assert!(chips(&rt)[0].state != mr_core::ChipState::Reading);
        let chip = chips(&rt)[0].id;
        assert!(rt.state.thumbs.contains_key(&chip));
        testkit::type_text(&mut rt, "Look");
        testkit::press(&mut rt, Key::Enter);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(chips(&rt).is_empty(), "the chip went with the message");
        assert_eq!(pictures(&rt, key), [chip]);
        let handle = rt
            .state
            .thumbs
            .get(&chip)
            .expect("held: the conversation holds its id")
            .clone();
        handle.wait().expect("made");
        testkit::step(&mut rt);
        let shown = format!("mr-user-picture-{chip}");
        assert!(testkit::element(&rt, &testkit::tile(key, &shown)).is_some());
    }

    #[test]
    fn pictures_go_with_their_session() {
        let mut rt = testkit::window("resting");
        rt.state.images = cache();
        let one = stored_with(&mut rt, "stored-one", with_image("One", 4, 3));
        testkit::show(&mut rt, one);
        pictures_come(&mut rt, one);
        let two = stored_with(&mut rt, "stored-two", with_image("Two", 3, 4));
        // Beside it, so that both are in a tile.
        testkit::show_side(&mut rt, two);
        pictures_come(&mut rt, two);
        let (a, b) = (pictures(&rt, one)[0], pictures(&rt, two)[0]);
        assert!(rt.state.thumbs.contains_key(&a) && rt.state.thumbs.contains_key(&b));
        rt.state.app.sessions.remove(two);
        run(&mut rt.state, Vec::new());
        assert!(!rt.state.thumbs.contains_key(&b), "gone with its session");
        assert!(
            rt.state.thumbs.contains_key(&a),
            "the other session's stays"
        );
    }

    #[test]
    fn the_pictures_of_a_session_that_let_go_are_dropped() {
        let mut rt = testkit::window("resting");
        rt.state.images = cache();
        let one = stored_with(&mut rt, "stored-one", with_image("One", 4, 3));
        testkit::show(&mut rt, one);
        pictures_come(&mut rt, one);
        let a = pictures(&rt, one)[0];
        assert!(rt.state.thumbs.contains_key(&a));
        let two = stored_with(&mut rt, "stored-two", with_image("Two", 3, 4));
        testkit::show(&mut rt, two);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, HistoryState::Loaded, "kept for now");
        assert!(rt.state.thumbs.contains_key(&a));
        // Ten minutes out of every tile, and the tick lets it go.
        rt.state.unix_clock = || demo::NOW_UNIX + mr_core::LET_GO_SECONDS;
        let effects = rt.state.app.tick((rt.state.unix_clock)());
        run(&mut rt.state, effects);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, HistoryState::NotLoaded);
        assert!(!rt.state.thumbs.contains_key(&a), "gone with its history");
    }

    /// A message of the user's stored as `m1`, with one image of 4 by 3 pixels the store
    /// gives again, and Claude's answer.
    fn with_stored_image() -> Vec<HistoryItem> {
        let mut items = with_image("Look", 4, 3);
        if let HistoryItem::User { stored, .. } = &mut items[0] {
            *stored = Some("m1".into());
        }
        items
    }

    #[test]
    fn a_second_answer_for_the_same_picture_changes_nothing() {
        let mut rt = testkit::window("resting");
        rt.state.images = cache();
        let key = stored_with(&mut rt, "stored-one", with_stored_image());
        testkit::show(&mut rt, key);
        pictures_come(&mut rt, key);
        let picture = pictures(&rt, key)[0];
        // Opened, closed and opened again before the first read answers: two reads are on
        // their way. The fake is out of the window meanwhile, so no frame waits for them.
        let fake = rt.state.fake.take().unwrap();
        fake.pause_reads();
        open_picture(&mut rt.state, key, picture);
        close_picture(&mut rt.state);
        open_picture(&mut rt.state, key, picture);
        assert!(rt.state.viewing.as_ref().unwrap().full.is_none());
        // The first answer is good.
        let image = |result| FromBackend::Image { picture, result };
        let good = || Ok(mr_agent::ImageData::new(testkit::png(4, 3)));
        take(&mut rt.state, image(good()), 0.0);
        let full = rt.state.viewing.as_ref().unwrap().full.clone();
        let full = full.expect("the full image's handle");
        full.clone().wait().expect("an image made in the test");
        testkit::step(&mut rt);
        assert!(rt.state.viewing.as_ref().unwrap().made, "it is drawn");
        // The second says the file is gone: the good image stays, and nothing says otherwise.
        take(
            &mut rt.state,
            image(Err(AgentError::Io("gone".into()))),
            0.0,
        );
        testkit::step(&mut rt);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert!(!viewing.failed);
        assert_eq!(viewing.full.as_ref().unwrap().size(), Some([4, 3]));
        assert!(!testkit::has_text(&rt, "The full picture cannot be read."));
        // A second good answer is dropped as well: what is drawn stays drawn.
        take(&mut rt.state, image(good()), 0.0);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert!(viewing.made && !viewing.failed);
        // The two reads that were under way answer at last, and change nothing either.
        fake.resume_reads();
        rt.state.fake = Some(fake);
        settle_backend(&mut rt);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert!(viewing.made && !viewing.failed);
        assert_eq!(viewing.full.as_ref().unwrap().size(), Some([4, 3]));
        // A picture whose first answer failed still takes a good one.
        close_picture(&mut rt.state);
        let fake = rt.state.fake.take().unwrap();
        fake.pause_reads();
        open_picture(&mut rt.state, key, picture);
        take(
            &mut rt.state,
            image(Err(AgentError::Io("gone".into()))),
            0.0,
        );
        assert!(rt.state.viewing.as_ref().unwrap().failed);
        take(&mut rt.state, image(good()), 0.0);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert!(viewing.full.is_some() && !viewing.failed);
        fake.resume_reads();
        rt.state.fake = Some(fake);
        settle_backend(&mut rt);
    }

    #[test]
    fn a_draft_survives_its_conversation_being_let_go() {
        let mut rt = testkit::window("resting");
        let one = stored_with(&mut rt, "stored-one", with_image("One", 4, 3));
        testkit::show(&mut rt, one);
        settle_backend(&mut rt);
        view_mut(&mut rt.state, one).draft = TextEdit::new("Half a thought");
        // Another session takes its tile; ten minutes later the tick lets the first one go.
        let two = stored_with(&mut rt, "stored-two", with_image("Two", 3, 4));
        testkit::show(&mut rt, two);
        settle_backend(&mut rt);
        let generation = rt.state.app.sessions.get(one).unwrap().session.generation;
        rt.state.unix_clock = || demo::NOW_UNIX + mr_core::LET_GO_SECONDS;
        let effects = rt.state.app.tick((rt.state.unix_clock)());
        run(&mut rt.state, effects);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, HistoryState::NotLoaded);
        assert!(session.generation > generation, "its items were replaced");
        assert_eq!(view_of(&rt.state, one).draft.text, "Half a thought");
        // Shown again, it is read again, and the draft is in its box.
        testkit::show(&mut rt, one);
        settle_backend(&mut rt);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, HistoryState::Loaded);
        assert_eq!(view_of(&rt.state, one).draft.text, "Half a thought");
    }

    #[test]
    fn a_second_launch_with_a_folder_opens_a_session_there() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("conversation");
        let before = testkit::focused(&rt);
        let folder = dir.path().to_path_buf();
        rt.update(|ui, tx| launched_again(ui, tx, Some(folder)));
        testkit::step(&mut rt);
        assert_ne!(testkit::focused(&rt), before);
        let shown = rt.state.focused_session().unwrap();
        assert_eq!(shown.folder, dir.path());
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert_eq!(
            typing_in(&rt.state),
            testkit::focused(&rt),
            "it takes the keys"
        );

        let sessions = rt.state.app.sessions.len();
        let focused = testkit::focused(&rt);
        rt.update(|ui, tx| launched_again(ui, tx, None));
        assert_eq!(rt.state.app.sessions.len(), sessions, "without a folder");
        assert_eq!(testkit::focused(&rt), focused);
    }

    #[test]
    fn with_a_folder_that_is_gone_it_opens_nothing() {
        let mut rt = testkit::window("conversation");
        rt.state.folder_exists = |_| false;
        let sessions = rt.state.app.sessions.len();
        let focused = testkit::focused(&rt);
        rt.update(|ui, tx| launched_again(ui, tx, Some("/home/alex/gone".into())));
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.sessions.len(), sessions);
        assert_eq!(testkit::focused(&rt), focused);
    }

    #[test]
    fn before_the_main_screen_a_second_launch_opens_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("signed-out");
        let sessions = rt.state.app.sessions.len();
        let folder = dir.path().to_path_buf();
        rt.update(|ui, tx| launched_again(ui, tx, Some(folder)));
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.sessions.len(), sessions);
        assert_ne!(rt.state.app.screen, Screen::Main);
    }

    #[test]
    fn a_launch_while_quitting_opens_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("conversation");
        rt.state.quitting = true;
        let sessions = rt.state.app.sessions.len();
        let focused = testkit::focused(&rt);
        let folder = dir.path().to_path_buf();
        rt.update(|ui, tx| launched_again(ui, tx, Some(folder)));
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.sessions.len(), sessions);
        assert_eq!(testkit::focused(&rt), focused);
    }
}
