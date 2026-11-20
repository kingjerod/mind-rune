const TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
/// The one `CLAUDE_` setting that is the user's own, not the parent session's.
const CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
/// Names without a `CLAUDE_` or `ANTHROPIC_` prefix that a parent Claude
/// session sets for its tools.
const INJECTED: [&str; 4] = [
    "CLAUDECODE",
    "AI_AGENT",
    "MCP_CONNECTION_NONBLOCKING",
    "MCP_SERVER_CONNECTION_BATCH_SIZE",
];

/// The environment a child `claude` is started with, from the parent's.
///
/// When the parent is itself a Claude session (it carries `CLAUDECODE` or
/// `CLAUDE_CODE_SESSION_ID`), the variables that session injected are removed:
/// `CLAUDECODE`, `AI_AGENT`, `MCP_CONNECTION_NONBLOCKING`,
/// `MCP_SERVER_CONNECTION_BATCH_SIZE`, everything starting `ANTHROPIC_`, and
/// everything starting `CLAUDE_` except `CLAUDE_CONFIG_DIR` and the OAuth
/// token. Otherwise everything passes through, including the user's own
/// `ANTHROPIC_*` settings. When the user has a full login the OAuth token is
/// removed in both cases, so Remote Control works. The order of the remaining
/// variables is kept.
pub fn child_env(parent: Vec<(String, String)>, has_full_login: bool) -> Vec<(String, String)> {
    let inside_claude = parent
        .iter()
        .any(|(key, _)| key == "CLAUDECODE" || key == "CLAUDE_CODE_SESSION_ID");
    parent
        .into_iter()
        .filter(|(key, _)| {
            if key == TOKEN {
                return !has_full_login;
            }
            !(inside_claude && injected_by_claude(key))
        })
        .collect()
}

/// The setting that gives the program its task tools (`TaskCreate` and the rest).
const TODO_TOOLS: &str = "CLAUDE_CODE_ENABLE_TODO_TOOLS";
/// The setting that, set to `0`, gives the older `TodoWrite` in their place.
const TASKS: &str = "CLAUDE_CODE_ENABLE_TASKS";

/// `env` with the program's task tools asked for: the user's own settings for them are
/// dropped and `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` is added last. The order of the rest is kept.
pub fn with_task_tools(env: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = env
        .into_iter()
        .filter(|(key, _)| key != TODO_TOOLS && key != TASKS)
        .collect();
    env.push((TODO_TOOLS.to_string(), "1".to_string()));
    env
}

fn injected_by_claude(key: &str) -> bool {
    INJECTED.contains(&key)
        || key.starts_with("ANTHROPIC_")
        || (key.starts_with("CLAUDE_") && key != CONFIG_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn variables_from_a_parent_claude_session_are_removed() {
        let parent = env(&[
            ("PATH", "/bin"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SESSION_ID", "x"),
            ("CLAUDE_CODE_SIMPLE", "1"),
            ("ANTHROPIC_BASE_URL", "http://x"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "t"),
            ("HOME", "/h"),
        ]);
        assert_eq!(
            child_env(parent, false),
            env(&[
                ("PATH", "/bin"),
                ("CLAUDE_CODE_OAUTH_TOKEN", "t"),
                ("HOME", "/h")
            ])
        );
    }

    #[test]
    fn claudecode_alone_marks_a_parent_claude_session() {
        let parent = env(&[
            ("PATH", "/bin"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_EFFORT", "high"),
            ("ANTHROPIC_MODEL", "m"),
        ]);
        assert_eq!(child_env(parent, false), env(&[("PATH", "/bin")]));
    }

    #[test]
    fn a_session_id_alone_marks_a_parent_claude_session() {
        let parent = env(&[
            ("CLAUDE_CODE_SESSION_ID", "x"),
            ("PATH", "/bin"),
            ("CLAUDE_PID", "42"),
            ("ANTHROPIC_BASE_URL", "http://x"),
        ]);
        assert_eq!(child_env(parent, false), env(&[("PATH", "/bin")]));
    }

    #[test]
    fn every_variable_a_parent_session_is_seen_to_inject_is_removed() {
        let parent = env(&[
            ("CLAUDECODE", "1"),
            ("CLAUDE_AGENT_SDK_VERSION", "0.1"),
            ("CLAUDE_EFFORT", "high"),
            ("CLAUDE_PID", "42"),
            ("AI_AGENT", "claude-code"),
            ("MCP_CONNECTION_NONBLOCKING", "1"),
            ("MCP_SERVER_CONNECTION_BATCH_SIZE", "4"),
            ("MCP_TIMEOUT", "9"),
            ("HOME", "/h"),
        ]);
        assert_eq!(
            child_env(parent, false),
            env(&[("MCP_TIMEOUT", "9"), ("HOME", "/h")])
        );
    }

    #[test]
    fn the_config_directory_is_kept_inside_a_claude_session() {
        let parent = env(&[
            ("CLAUDECODE", "1"),
            ("CLAUDE_CONFIG_DIR", "/h/.claude-work"),
            ("PATH", "/bin"),
        ]);
        assert_eq!(
            child_env(parent, false),
            env(&[("CLAUDE_CONFIG_DIR", "/h/.claude-work"), ("PATH", "/bin")])
        );
    }

    #[test]
    fn a_plain_shell_keeps_the_users_own_settings() {
        let parent = env(&[
            ("PATH", "/bin"),
            ("ANTHROPIC_API_KEY", "k"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "t"),
        ]);
        assert_eq!(child_env(parent.clone(), false), parent);
    }

    #[test]
    fn the_token_is_dropped_when_a_full_login_exists() {
        let parent = env(&[("PATH", "/bin"), ("CLAUDE_CODE_OAUTH_TOKEN", "t")]);
        assert_eq!(child_env(parent, true), env(&[("PATH", "/bin")]));
    }

    #[test]
    fn the_token_is_dropped_with_a_full_login_inside_a_claude_session_too() {
        let parent = env(&[
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "t"),
            ("PATH", "/bin"),
        ]);
        assert_eq!(child_env(parent, true), env(&[("PATH", "/bin")]));
    }

    #[test]
    fn the_task_tools_are_asked_for_once_whatever_the_shell_says() {
        let given = env(&[
            ("PATH", "/bin"),
            ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "0"),
            ("HOME", "/home/alex"),
            ("CLAUDE_CODE_ENABLE_TASKS", "0"),
        ]);
        assert_eq!(
            with_task_tools(given),
            env(&[
                ("PATH", "/bin"),
                ("HOME", "/home/alex"),
                ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")
            ])
        );
        assert_eq!(
            with_task_tools(Vec::new()),
            env(&[("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")])
        );
    }
}
