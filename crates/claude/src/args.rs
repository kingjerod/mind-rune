use crate::mode::mode_to_wire;
use mr_agent::{PermissionMode, SessionId, StartSpec};

/// The command-line arguments for one `claude` session.
///
/// `new_id` is the id the adapter chose for a session that is started fresh or
/// forked from a resumed one; a plain resume keeps its own id and ignores it.
/// With the `Ask` mode the `--permission-mode` flag is left off, so the user's
/// own Claude Code settings decide.
pub fn build_args(spec: &StartSpec, new_id: Option<&SessionId>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(resume) = &spec.resume {
        args.push("--resume".into());
        args.push(resume.0.clone());
        if spec.fork {
            args.push("--fork-session".into());
            if let Some(id) = new_id {
                args.push("--session-id".into());
                args.push(id.0.clone());
            }
        }
    } else if let Some(id) = new_id {
        args.push("--session-id".into());
        args.push(id.0.clone());
    }
    if let Some(model) = &spec.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(mode) = &spec.permission_mode
        && *mode != PermissionMode::Ask
    {
        args.push("--permission-mode".into());
        args.push(mode_to_wire(mode));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: [&str; 9] = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
    ];

    fn with(extra: &[&str]) -> Vec<String> {
        BASE.iter().chain(extra).map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_new_session_is_given_its_id() {
        let id = SessionId("11111111-1111-4111-8111-111111111111".into());
        assert_eq!(
            build_args(&StartSpec::new("/p"), Some(&id)),
            with(&["--session-id", "11111111-1111-4111-8111-111111111111"])
        );
    }

    #[test]
    fn a_plain_resume_keeps_its_id_and_ignores_the_new_one() {
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(SessionId("abc".into()));
        let id = SessionId("unused".into());
        assert_eq!(build_args(&spec, Some(&id)), with(&["--resume", "abc"]));
        assert_eq!(build_args(&spec, None), with(&["--resume", "abc"]));
    }

    #[test]
    fn a_fork_is_given_the_new_id() {
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(SessionId("abc".into()));
        spec.fork = true;
        let id = SessionId("22222222-2222-4222-8222-222222222222".into());
        assert_eq!(
            build_args(&spec, Some(&id)),
            with(&[
                "--resume",
                "abc",
                "--fork-session",
                "--session-id",
                "22222222-2222-4222-8222-222222222222"
            ])
        );
    }

    #[test]
    fn model_and_mode_follow_and_ask_leaves_the_mode_flag_off() {
        let mut spec = StartSpec::new("/p");
        spec.model = Some("opus".into());
        spec.permission_mode = Some(PermissionMode::Plan);
        assert_eq!(
            build_args(&spec, None),
            with(&["--model", "opus", "--permission-mode", "plan"])
        );
        spec.permission_mode = Some(PermissionMode::Ask);
        assert_eq!(build_args(&spec, None), with(&["--model", "opus"]));
        spec.permission_mode = Some(PermissionMode::Auto);
        assert_eq!(
            build_args(&spec, None),
            with(&["--model", "opus", "--permission-mode", "auto"])
        );
    }
}
