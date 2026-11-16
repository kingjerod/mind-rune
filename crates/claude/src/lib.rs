//! The adapter that runs the `claude` command-line program as a child process
//! and speaks its stream-json protocol.

mod account;
mod agent;
mod args;
mod encode;
mod env;
mod history;
mod history_tail;
mod mode;
mod process;
mod time;
mod translate;
mod wire;

pub use account::{StatusInfo, detect, parse_status, plan_label};
pub use agent::ClaudeAgent;
pub use args::build_args;
pub use encode::{
    PermissionReply, control_request, generate_session_title, get_context_usage, get_settings,
    get_usage, initialize, interrupt, permission_reply, remote_control, rename_session, set_effort,
    set_model, set_permission_mode, unsupported_reply, user_message,
};
pub use env::child_env;
pub use history::{
    DescriptionTable, StoredEntry, StoredMeta, describe, find_session, list_entries,
    stored_sessions,
};
pub use history_tail::{TAIL_BYTES, read_tail};
pub use mode::{mode_from_wire, mode_to_wire};
pub use process::{ClaudeProcess, ExitInfo, SpawnConfig};
pub use translate::{SessionStart, Tracked, Translator, always_allow_label, tool_detail};
pub use wire::{Block, BlockKind, Incoming, StreamEvent, ToolResult, parse_line};
