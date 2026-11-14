//! Everything the window knows, as plain data: no UI code and no adapter code.

mod app;
mod backend;
mod conversation;
mod session;
mod tools;
mod usage;

pub use app::{App, DEFAULT_WIDTH, Effect, MIN_WIDTH, Screen, Width};
pub use backend::{Backend, FromBackend, ToBackend, Wake};
pub use conversation::{
    Conversation, DID_NOT_FINISH, Item, ItemBody, ItemId, NoticeKind, RequestState, thinking_label,
};
pub use session::{Phase, Session, answer_words, mode_label, model_option_label};
pub use tools::{
    Clip, OUTPUT_CHARS, OUTPUT_LINES, ToolCall, ToolFamily, ToolGroup, clip, family, fenced,
    file_name, line_counts, unified_diff,
};
pub use usage::{Level, UsageLine, account_line, resets_in, usage_lines};
