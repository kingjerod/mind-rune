//! The window's state, and the glue between it and the core: effects are
//! carried out here and the backend's messages are taken in here, so the
//! views only read `Ui` and call the `App`'s intents.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mr_agent::{Agent, FakeAgent, RequestId};
#[cfg(test)]
use mr_core::EntryMut;
use mr_core::{
    App, Backend, DEFAULT_WIDTH, Effect, FromBackend, GroupId, ItemBody, ItemId, Launch, Leaving,
    Line, Phase, Place, RequestState, SAVED_VERSION, Saved, Screen, Session, SessionKey, Spot,
    ToBackend, Width,
};
use rune::prefs::Prefs;
use rune::runtime::Waker;
use rune::{PointerPress, TextEdit, Tx};
use rune_kit::shell::PaneState;

use crate::demo;
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

/// Which of the input box's pickers has its list open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Model,
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

pub struct Ui {
    pub app: App,
    pub agent: Arc<dyn Agent>,
    /// Present in the demo window and in tests: the agent, as the fake it is.
    pub fake: Option<FakeAgent>,
    pub backend: Option<Backend>,
    pub prefs: Prefs,
    pub sidebar: PaneState,
    /// How each session shown in this run is shown, by its key. A session that is gone loses
    /// its view at the next [`run`].
    pub views: HashMap<SessionKey, SessionView>,
    /// Where the wall clock is read from: `app.now_unix` is set from it on every poll.
    pub unix_clock: fn() -> i64,
    /// Whether a folder is there, asked when a message would wake a resting session in it.
    /// The demo's folders are made up, so the demo says yes to every one.
    pub folder_exists: fn(&Path) -> bool,
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
    /// button until the next try.
    pub folder_error: Option<String>,
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
    /// sidebar's width and whether it is collapsed are applied to the sidebar, the rest goes to
    /// the app.
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
            views: HashMap::new(),
            unix_clock,
            folder_exists: Path::is_dir,
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
            swallow_row_press: false,
            passed_row: None,
            renaming: None,
            rename: TextEdit::new(""),
            folder_menu: None,
            folder_side: false,
            folder_offers: Vec::new(),
            folder_error: None,
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
/// in its input box, else forgotten when it is empty. `Side`, `At`, `Above` and `Below` take
/// none out, but `Side` swaps the focused tile's session when there is no room, so it is read
/// as `Focused`.
pub fn leaving(ui: &Ui, place: Place) -> Leaving {
    let out = match place {
        Place::Focused | Place::Side => ui.app.focused(),
        Place::Tile(key) => Some(key),
        Place::At(_) | Place::Above(_) | Place::Below(_) => None,
    };
    let typed = out.is_some_and(|left| {
        view_of(ui, left)
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
    let tiled = ui.app.tiles.contains(key);
    let effects = ui.app.show(key, place, leaving(ui, place));
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
fn type_in(ui: &mut Ui, key: SessionKey) -> bool {
    let typing = ui.app.tiles.contains(key)
        && ui
            .app
            .sessions
            .get(key)
            .is_some_and(|entry| !ended(&entry.session));
    if typing {
        view_mut(ui, key).draft.focused = true;
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

/// `AppSpec::setup`: starts the backend with the window's waker and carries out what was waiting.
/// A second call does nothing, so a scene that started its own backend keeps it.
pub fn setup(ui: &mut Ui, waker: Waker) {
    if ui.backend.is_some() {
        return;
    }
    let backend = Backend::start(ui.agent.clone(), waker);
    for message in ui.waiting.drain(..) {
        backend.send(message);
    }
    ui.backend = Some(backend);
}

/// `AppSpec::poll`. The app's clock is brought up to date first, so what is taken in after is
/// stamped with it. Tiles that come or go with what was taken in (a row restored, a session
/// gone) do so at once, as when the user opens or closes one.
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
    if ui.was_tiled != tiled {
        tx.instant();
        changed = true;
    }
    changed
}

/// Carries effects out: backend messages are sent, the width is saved, and so is the state kept
/// between runs, with the sidebar's width and whether it is collapsed. A look in a folder that
/// is not there runs in the home folder. Messages for the backend wait until `setup` has
/// started it.
///
/// Every change to the `App` (an intent, or a message taken in) is followed by a call to this,
/// so it ends by bringing the sidebar's model up to date (`App::settle`, which also takes out
/// the tiles of sessions that are gone) and the sessions' views with it ([`tidy_views`]).
///
/// Last, the rule of ruling R7: while a text field of a tile holds the keyboard focus, that
/// tile is the focused one. An intent on a session in another tile (a request button, a row's
/// toggle: controls that take no keyboard focus) focused that other tile; the focus goes back
/// to the tile the user types in here, in the same change, so no outline or row flashes. A
/// tile focused on purpose whose box cannot take the keyboard leaves no field holding it
/// ([`focus_tile`]), so nothing is given back.
pub fn run(ui: &mut Ui, effects: Vec<Effect>) {
    for effect in effects {
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
            Effect::CheckActivity { key, id } => ToBackend::CheckActivity { key, id },
            // A folder that is gone (a removed worktree, say) would start nothing and learn
            // nothing: the look runs at home instead.
            Effect::Look { folder } if !folder.is_dir() => ToBackend::Look {
                folder: ui.app.home().to_path_buf(),
            },
            Effect::Look { folder } => ToBackend::Look { folder },
        };
        match &ui.backend {
            Some(backend) => backend.send(message),
            None => ui.waiting.push(message),
        }
    }
    ui.app.settle();
    tidy_views(ui);
    tidy_sidebar(ui);
    tidy_handles(ui);
    if let Some(typing) = typing_in(ui)
        && ui.app.focused() != Some(typing)
    {
        let effects = ui.app.focus_tile(typing);
        run(ui, effects);
    }
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

/// The sidebar's menu shuts when what it is the menu of is gone, and so does the folder menu of
/// a group no longer listed. A row's drag ends when its session is gone, a header's when its
/// tile is, and either while a menu is open: a menu takes the pointer, and the drag's release
/// never comes.
///
/// A rename whose field is gone with its place ends: that of a group no longer listed, of a
/// session that is gone, of a row no longer listed, of a header whose session left its tile.
/// It ends dropped while the field still had the keyboard focus (the tile closed under it, say);
/// a session's name is kept when the field had already lost it (a click on another row shows
/// that session in the header's tile), as `view::sidebar::rename_watch` would have kept it.
fn tidy_sidebar(ui: &mut Ui) {
    let model = &ui.app.sidebar;
    let listed = |id: &GroupId| model.sections.iter().any(|section| section.id == *id);
    let gone = match ui.side_menu.as_ref().map(|menu| &menu.target) {
        Some(MenuTarget::Row(key)) => ui.app.sessions.get(*key).is_none(),
        Some(MenuTarget::Group(id)) => !listed(id),
        Some(MenuTarget::HiddenGroups) => model.hidden.is_empty(),
        None => false,
    };
    if gone {
        ui.side_menu = None;
    }
    let menu = ui.side_menu.is_some() || ui.folder_menu.is_some();
    if let Some(drag) = &ui.drag {
        let gone = match drag.from {
            DragFrom::Row => ui.app.sessions.get(drag.key).is_none(),
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
fn tidy_views(ui: &mut Ui) {
    let Ui {
        app,
        views,
        was_tiled,
        ..
    } = ui;
    views.retain(|key, _| app.sessions.get(*key).is_some());
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
/// has ended; for the model picker, also when there are no models to choose from), so it does not
/// open again by itself once it can.
fn shut_disabled_picker(view: &mut SessionView, session: &Session) {
    let usable = !matches!(session.phase, Phase::Opening | Phase::Ended { .. });
    if !usable || (view.open_picker == Some(Picker::Model) && session.models.is_empty()) {
        view.open_picker = None;
    }
}

/// How long closing the window waits for its sessions to end, all of them together: a little
/// more than the adapter gives `claude` after its input closes before it kills it, so their end
/// is always seen.
const CLOSE_WAIT: Duration = Duration::from_millis(2500);

/// `App::on_close`: the window is closing, and the app with it. Ends every session, waiting up
/// to [`CLOSE_WAIT`] for them, so no program that runs one is left going without a window.
pub fn closing(ui: &mut Ui, _tx: &mut Tx) {
    if let Some(warning) = close_warning(ui, CLOSE_WAIT) {
        eprintln!("{warning}");
    }
}

/// Ends the sessions as [`close_session`] does, and says so when one had not ended after `limit`.
/// The demo's fake is only told to close, never waited for: nothing is wrong there, so there is
/// nothing to say.
pub(crate) fn close_warning(ui: &mut Ui, limit: Duration) -> Option<String> {
    if close_session(ui, limit) || ui.fake.is_some() {
        return None;
    }
    Some(format!(
        "mindrune: the session had not ended {} ms after the window closed",
        limit.as_millis()
    ))
}

/// Writes the state kept between runs, then sends `Close` for every session whose program has
/// not ended and waits up to `limit` in all for them to end. True when no session is left
/// running. The demo's fake is only told to close: its time does not pass unless it is pumped,
/// so nothing would come.
///
/// This wait is the one exception to the rule that the window's thread never blocks. It is
/// bounded, and it happens only as the window closes: once the app has quit, nothing would be
/// left to see that the program stops, and the adapter's own kill after its grace period runs
/// on a thread that dies with the app.
pub(crate) fn close_session(ui: &mut Ui, limit: Duration) -> bool {
    let effects = ui.app.close((ui.unix_clock)());
    let closing = effects
        .iter()
        .any(|effect| matches!(effect, Effect::Send { .. }));
    // The state is written here, before the wait, so it is kept even if the wait is cut short.
    run(ui, effects);
    if !closing {
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
        let effects = ui.app.receive(message, now);
        run(ui, effects);
    }
    took
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo, testkit};
    use mr_agent::{
        Account, AgentError, Capabilities, EventSink, FakeAgent, HistoryItem, PermissionMode,
        SessionCommand, SessionEvent, SessionHandle, SessionId, SessionInfo, StartSpec,
        StoredHistory, StoredSession, ToolCallId, ToolDetail, ToolOutcome,
    };
    use mr_core::{HistoryState, ItemBody, Phase, Screen, ThinkingTime, Width};
    use rune::prefs::Prefs;
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
    fn closing_the_demo_window_closes_its_session_without_waiting() {
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
    fn closing_the_demo_window_has_nothing_to_report() {
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
    fn closing_the_window_waits_for_the_session_to_end() {
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
    fn closing_the_window_waits_no_longer_than_its_limit() {
        let agent = Arc::new(SlowExit {
            exit_after: None,
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let limit = Duration::from_millis(200);
        let started = Instant::now();
        assert_eq!(
            close_warning(&mut ui, limit).as_deref(),
            Some("mindrune: the session had not ended 200 ms after the window closed")
        );
        let waited = started.elapsed();
        assert!(
            waited >= limit && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1);
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
                closed_by_app: Vec::new(),
                ..written
            }
        );
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
            }],
            ..StoredHistory::default()
        };
        let effects = ui.app.receive(
            FromBackend::History {
                key: a,
                result: Ok(history),
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
    fn closing_ends_every_session_and_saves() {
        let mut rt = testkit::window("empty");
        for folder in ["/home/alex/a", "/home/alex/b", "/home/alex/c"] {
            testkit::add_session(&mut rt, folder);
        }
        assert_eq!(rt.state.app.sessions.len(), 3);
        let fake = rt.state.fake.clone().unwrap();
        // The scene's own session, forgotten, was closed already.
        let before = fake.sent().len();
        rt.state.prefs.remove(STATE_KEY).unwrap();
        close_session(&mut rt.state, CLOSE_WAIT);
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
}
