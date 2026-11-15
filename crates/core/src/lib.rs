//! Everything the window knows, as plain data: no UI code and no adapter code.

mod app;
mod backend;
mod conversation;
mod groups;
mod saved;
mod session;
mod sessions;
mod tiles;
mod tools;
mod usage;

pub use app::{
    App, DEFAULT_WIDTH, ELSEWHERE_SECONDS, Effect, HISTORY_ITEMS, Launch, Leaving, Listing,
    MIN_WIDTH, OWN_EXIT_SLACK_SECONDS, PromptChoice, RECENT_DAYS, RELIST_SECONDS, Screen,
    USAGE_RETRY_SECONDS, Width,
};
pub use backend::{Backend, FromBackend, ToBackend, Wake};
pub use conversation::{
    Conversation, DID_NOT_FINISH, Item, ItemBody, ItemId, NoticeKind, RequestState, ThinkingTime,
    thinking_label,
};
pub use groups::{Group, GroupId, Groups, Line, Row, Section, SidebarModel, sidebar_model};
pub use saved::{RECENT_FOLDERS, SAVED_VERSION, Saved, SavedTile};
pub use session::{HistoryState, Phase, Session, answer_words, mode_label, model_option_label};
pub use sessions::{Entry, EntryMut, Mark, Prompt, SessionKey, Sessions};
pub use tiles::{MIN_TILE_WIDTH, Place, Tile, Tiles};
pub use tools::{
    Clip, OUTPUT_CHARS, OUTPUT_LINES, ToolCall, ToolFamily, ToolGroup, clip, family, fenced,
    file_name, line_counts, unified_diff,
};
pub use usage::{Level, UsageLine, account_line, resets_in, usage_lines};
