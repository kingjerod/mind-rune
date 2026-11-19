//! Removing one stored session for good: the only place the adapter changes
//! anything inside Claude Code's folder.
//!
//! A session leaves at most four kinds of path, each named by its id:
//! `projects/<project>/<id>.jsonl`, the folder `projects/<project>/<id>` beside it,
//! `file-history/<id>` and `session-env/<id>`. Nothing else is touched. Only an id
//! in the written form of a UUID names anything, only an absolute root that holds a
//! `projects` is a store, and a link in place of a session's own path is removed as a
//! link. Where no store is found the answer is an error, never "the session is gone".

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use mr_agent::{AgentError, Removal, SessionId};

use crate::history::modified_unix;

/// The paths of one session that exist, its transcripts apart from the rest.
#[derive(Default)]
struct Found {
    transcripts: Vec<PathBuf>,
    others: Vec<PathBuf>,
}

/// The store's own paths of one session, those that exist. `root` is Claude Code's folder.
///
/// A root that is no store (see [`remove`]) answers the error "the store could not be
/// found", and a store that cannot be read "the store could not be read": neither is ever
/// an empty list. An id that is no UUID names no path.
pub fn paths_of(root: &Path, id: &SessionId) -> Result<Vec<PathBuf>, AgentError> {
    let found =
        find(store(root)?, id).map_err(|_| AgentError::Io("the store could not be read".into()))?;
    Ok(found.transcripts.into_iter().chain(found.others).collect())
}

/// Removes every path of session `id`, unless a transcript of it was modified later than
/// `unless_after_unix`. The transcripts go last, so a session that could not be removed whole
/// is still listed.
///
/// Links above the session's own paths are followed, as reading follows them: the root,
/// `projects`, a project folder, `file-history` and `session-env` may each be a link, and
/// the session is removed where they point. The session's own file and folders are never
/// followed: a link there is removed as a link.
///
/// `Missing` is answered only by a store that was looked through and holds no transcript of
/// the session. A root that is not absolute or not there, and one with no `projects` in it
/// (a folder not mounted, a link to nowhere), is no store: the error "the store could not be
/// found", and nothing is touched. An id that is no UUID is an error too. In each case the
/// session is not known to be gone.
pub fn remove(root: &Path, id: &SessionId, unless_after_unix: i64) -> Result<Removal, AgentError> {
    let root = store(root)?;
    if !is_uuid(&id.0) {
        return Err(not_removed(()));
    }
    let found = find(root, id).map_err(not_removed)?;
    if changed_after(&found.transcripts, unless_after_unix)? {
        return Ok(Removal::Changed);
    }
    for path in &found.others {
        remove_path(path).map_err(not_removed)?;
    }
    // Read again, so that the session cannot have been used again while the side folders
    // went. If it was, they are gone already; that is the smaller loss, for a transcript
    // must never go after a change.
    if changed_after(&found.transcripts, unless_after_unix)? {
        return Ok(Removal::Changed);
    }
    for path in &found.transcripts {
        remove_path(path).map_err(not_removed)?;
    }
    Ok(if found.transcripts.is_empty() {
        Removal::Missing
    } else {
        Removal::Removed
    })
}

/// Claude Code's folder: `CLAUDE_CONFIG_DIR` when set and not empty, else `.claude` in the
/// home folder. Only an absolute folder counts: a relative one would depend on the working
/// folder.
pub fn root_from(config_dir: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    let root = match config_dir {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home?.join(".claude"),
    };
    root.is_absolute().then_some(root)
}

/// `root` when it is absolute and holds a `projects`, else the error of a store that cannot
/// be found. A `projects` that is there and cannot be read is found: reading it says so.
fn store(root: &Path) -> Result<&Path, AgentError> {
    let no_projects = || {
        matches!(
            std::fs::metadata(root.join("projects")),
            Err(e) if e.kind() == ErrorKind::NotFound
        )
    };
    if !root.is_absolute() || no_projects() {
        Err(no_store())
    } else {
        Ok(root)
    }
}

/// The error when there is no store to look in, which is never taken for a session that is gone.
pub(crate) fn no_store() -> AgentError {
    AgentError::Io("the store could not be found".into())
}

/// Whether any of `transcripts` was modified later than `unless_after_unix`.
fn changed_after(transcripts: &[PathBuf], unless_after_unix: i64) -> Result<bool, AgentError> {
    for transcript in transcripts {
        if modified_unix(transcript)
            .map_err(not_removed)?
            .is_some_and(|unix| unix > unless_after_unix)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `id` is a UUID in its written form: 8, 4, 4, 4 and 12 hexadecimal digits
/// joined by dashes, and nothing else.
fn is_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// Looks in every project folder, as `find_session` does, and in the two folders named by
/// session ids. A projects folder that cannot be listed, also one gone since [`store`]
/// looked, is an error. An id that is no UUID names nothing.
fn find(root: &Path, id: &SessionId) -> io::Result<Found> {
    let mut found = Found::default();
    if !is_uuid(&id.0) {
        return Ok(found);
    }
    let transcript_name = format!("{}.jsonl", id.0);
    for project in std::fs::read_dir(root.join("projects"))? {
        let project = project?.path();
        found
            .transcripts
            .extend(existing(project.join(&transcript_name))?);
        found.others.extend(existing(project.join(&id.0))?);
    }
    for kind in ["file-history", "session-env"] {
        found.others.extend(existing(root.join(kind).join(&id.0))?);
    }
    Ok(found)
}

/// `path` when something, a link included, is there. A file where a folder was looked for
/// holds nothing.
fn existing(path: PathBuf) -> io::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(path)),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Removes a real folder with all it holds, and a file or a link as itself. Something
/// already gone counts as removed.
fn remove_path(path: &Path) -> io::Result<()> {
    let removed = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) => Err(e),
    };
    match removed {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The error of a removal, which names no path.
fn not_removed<E>(_: E) -> AgentError {
    AgentError::Io("the session could not be removed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::{Duration, UNIX_EPOCH};

    const ID: &str = "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f";
    const OTHER: &str = "1c6e4d3f-7b2e-4d9f-a031-8e4c2b3d5f60";
    /// The modification time every transcript is given, in Unix seconds.
    const T: i64 = 1_700_000_000;

    fn id(text: &str) -> SessionId {
        SessionId(text.into())
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn set_modified(path: &Path, unix: i64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(unix as u64))
            .unwrap();
    }

    /// Writes `projects/<project>/<id>.jsonl`, modified at `unix`.
    fn transcript(root: &Path, project: &str, id: &str, unix: i64) -> PathBuf {
        let path = root
            .join("projects")
            .join(project)
            .join(format!("{id}.jsonl"));
        write(&path, "{\"type\":\"user\"}\n");
        set_modified(&path, unix);
        path
    }

    /// Every path under `root` without following a link: a file with its bytes, a link
    /// with its target, a folder with nothing.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut seen = BTreeMap::new();
        let mut todo = vec![root.to_path_buf()];
        while let Some(dir) = todo.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
                let content = if kind.is_symlink() {
                    Some(
                        std::fs::read_link(&path)
                            .unwrap()
                            .into_os_string()
                            .into_encoded_bytes(),
                    )
                } else if kind.is_dir() {
                    todo.push(path.clone());
                    None
                } else {
                    Some(std::fs::read(&path).unwrap())
                };
                seen.insert(path, content);
            }
        }
        seen
    }

    fn exists(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// A whole session: its transcript in `project`, a folder beside it with a sub-agent's
    /// file, its file history and its (empty) environment folder.
    fn whole_session(root: &Path, project: &str, id: &str) -> Vec<PathBuf> {
        let jsonl = transcript(root, project, id, T);
        let beside = root.join("projects").join(project).join(id);
        write(&beside.join("subagents/agent-a1.jsonl"), "{}\n");
        let history = root.join("file-history").join(id);
        write(&history.join("0a1b@v1"), "old text");
        let env = root.join("session-env").join(id);
        std::fs::create_dir_all(&env).unwrap();
        vec![jsonl, beside, history, env]
    }

    fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort();
        paths
    }

    #[test]
    fn only_the_written_form_of_a_uuid_is_an_id() {
        assert!(is_uuid(ID));
        assert!(is_uuid("0B5F3C2E-6A1D-4C8E-9F20-7D3B1A2C4E5F"));
        for not in [
            "",
            ".",
            "..",
            "a/b",
            "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5",
            "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f0",
            "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5g",
            "0b5f3c2e6-a1d-4c8e-9f20-7d3b1a2c4e5f",
            "0b5f3c2e-6a1d-4c8e-9f20_7d3b1a2c4e5f",
            "0b5f3c2e06a1d04c8e09f2007d3b1a2c4e5f",
            "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f ",
            " 0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f",
            "{0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5}",
            "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4eé",
            "0b5f3c2e6a1d4c8e9f207d3b1a2c4e5f",
        ] {
            assert!(!is_uuid(not), "{not:?} is no UUID");
        }
    }

    #[test]
    fn the_four_kinds_of_path_are_found() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let expected = whole_session(root, "-home-alex-a", ID);
        transcript(root, "-home-alex-b", OTHER, T);
        assert_eq!(sorted(paths_of(root, &id(ID)).unwrap()), sorted(expected));
        assert_eq!(
            paths_of(root, &id(OTHER)).unwrap(),
            vec![
                root.join("projects/-home-alex-b")
                    .join(format!("{OTHER}.jsonl"))
            ]
        );
        assert!(
            paths_of(root, &id("2d7f5e40-8c3f-4ea0-b142-9f5d3c4e6071"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_session_is_removed_with_its_folders_and_nothing_else() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let gone = whole_session(root, "-home-alex-a", ID);
        whole_session(root, "-home-alex-b", OTHER);
        write(
            &root.join("projects/-home-alex-b/memory/MEMORY.md"),
            "notes",
        );
        write(&root.join("history.jsonl"), "{\"display\":\"hello\"}\n");
        std::fs::create_dir_all(root.join("projects/-home-alex-empty")).unwrap();
        let before = snapshot(root);

        // A time equal to the bound is not later than it.
        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));

        let expected: BTreeMap<_, _> = before
            .into_iter()
            .filter(|(path, _)| !gone.iter().any(|g| path.starts_with(g)))
            .collect();
        assert_eq!(snapshot(root), expected);
        assert!(root.join("projects/-home-alex-a").is_dir());
        assert!(paths_of(root, &id(ID)).unwrap().is_empty());
    }

    #[test]
    fn a_session_that_changed_since_is_left() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        whole_session(root, "-home-alex-a", ID);
        let before = snapshot(root);
        assert_eq!(remove(root, &id(ID), T - 1), Ok(Removal::Changed));
        assert_eq!(snapshot(root), before);
    }

    #[test]
    fn a_session_that_is_gone_counts_as_missing() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let history = root.join("file-history").join(ID);
        write(&history.join("0a1b@v1"), "old text");
        transcript(root, "-home-alex-b", OTHER, T);
        let before = snapshot(root);

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Missing));

        assert!(!exists(&history));
        assert!(root.join("file-history").is_dir());
        let expected: BTreeMap<_, _> = before
            .into_iter()
            .filter(|(path, _)| !path.starts_with(&history))
            .collect();
        assert_eq!(snapshot(root), expected);
        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Missing));
    }

    #[test]
    fn an_id_that_is_no_uuid_removes_nothing() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        whole_session(root, "-home-alex-a", ID);
        let not_hex = "0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5g";
        let spaced = format!("{ID} ");
        // Paths such ids would name if they were taken as they are.
        transcript(root, "-home-alex-a", not_hex, T);
        transcript(root, "-home-alex-a", &spaced, T);
        transcript(root, "-home-alex-a", "", T);
        transcript(root, "-home-alex-a", "..", T);
        write(
            &root.join("file-history").join(not_hex).join("0a1b@v1"),
            "old text",
        );
        write(&root.join("session-env").join(&spaced).join("x"), "");
        let before = snapshot(root);

        // Never `Missing`: that would let the app forget a deletion nobody carried out.
        let not_removed = AgentError::Io("the session could not be removed".into());
        for text in ["..", "a/b", "", not_hex, spaced.as_str()] {
            assert!(paths_of(root, &id(text)).unwrap().is_empty(), "{text:?}");
            assert_eq!(
                remove(root, &id(text), T),
                Err(not_removed.clone()),
                "{text:?}"
            );
        }
        assert_eq!(snapshot(root), before);
    }

    #[test]
    fn a_root_that_is_not_there_is_no_store() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("not-made-yet");
        let no_store = AgentError::Io("the store could not be found".into());
        assert_eq!(remove(&root, &id(ID), T), Err(no_store.clone()));
        assert_eq!(paths_of(&root, &id(ID)), Err(no_store));
        assert!(!exists(&root));
    }

    #[test]
    fn without_projects_nothing_is_removed_and_the_session_is_not_missing() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        write(
            &root.join("file-history").join(ID).join("0a1b@v1"),
            "old text",
        );
        std::fs::create_dir_all(root.join("session-env").join(ID)).unwrap();
        let before = snapshot(root);
        let no_store = AgentError::Io("the store could not be found".into());

        assert_eq!(remove(root, &id(ID), T), Err(no_store.clone()));
        assert_eq!(paths_of(root, &id(ID)), Err(no_store.clone()));
        assert_eq!(snapshot(root), before, "the side folders stay");

        // A link to a folder that is not there, as to a volume not mounted.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("elsewhere"), root.join("projects")).unwrap();
            let before = snapshot(root);
            assert_eq!(remove(root, &id(ID), T), Err(no_store));
            assert_eq!(snapshot(root), before);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_folder_is_removed_and_its_target_stays() {
        use std::os::unix::fs::symlink;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("kept");
        write(&target.join("keep.txt"), "still here");
        let target_jsonl = elsewhere.path().join("kept.jsonl");
        write(&target_jsonl, "{\"type\":\"user\"}\n");
        set_modified(&target_jsonl, T);

        let project = root.join("projects/-home-alex-a");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(root.join("file-history")).unwrap();
        let links = [
            project.join(format!("{ID}.jsonl")),
            project.join(ID),
            root.join("file-history").join(ID),
        ];
        symlink(&target_jsonl, &links[0]).unwrap();
        symlink(&target, &links[1]).unwrap();
        symlink(&target, &links[2]).unwrap();
        let outside = snapshot(elsewhere.path());

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));

        for link in &links {
            assert!(!exists(link), "{link:?} is still there");
        }
        assert_eq!(snapshot(elsewhere.path()), outside);
        assert_eq!(
            std::fs::read_to_string(target.join("keep.txt")).unwrap(),
            "still here"
        );
    }

    #[test]
    fn the_same_id_in_two_projects_goes_from_both() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let mut paths = whole_session(root, "-home-alex-a", ID);
        let older = transcript(root, "-home-alex-b", ID, T - 100);
        let beside = root.join("projects/-home-alex-b").join(ID);
        write(&beside.join("custom-title.json"), "{}");
        paths.extend([older, beside]);
        assert_eq!(
            sorted(paths_of(root, &id(ID)).unwrap()),
            sorted(paths.clone())
        );

        // The newest of the two decides.
        assert_eq!(remove(root, &id(ID), T - 50), Ok(Removal::Changed));
        assert!(paths.iter().all(|path| exists(path)));

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));
        assert!(paths.iter().all(|path| !exists(path)));
        assert!(root.join("projects/-home-alex-a").is_dir());
        assert!(root.join("projects/-home-alex-b").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_removed_is_an_error_without_a_path() {
        use std::os::unix::fs::PermissionsExt;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let paths = whole_session(root, "-home-alex-a", ID);
        let locked = root.join("file-history");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root may write in a read-only folder, so there is nothing to see then.
        let is_root = std::fs::write(locked.join("probe"), "").is_ok();

        let result = remove(root, &id(ID), T);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if is_root {
            return;
        }
        assert_eq!(
            result,
            Err(AgentError::Io("the session could not be removed".into()))
        );
        // The transcript goes last, so a session that could not be removed is still listed.
        assert!(exists(&paths[0]));
    }

    #[test]
    fn the_store_is_where_the_program_looks() {
        let home = Path::new("/home/alex");
        assert_eq!(
            root_from(Some(OsStr::new("/x/cfg")), Some(home)),
            Some(PathBuf::from("/x/cfg"))
        );
        assert_eq!(
            root_from(Some(OsStr::new("")), Some(home)),
            Some(PathBuf::from("/home/alex/.claude"))
        );
        assert_eq!(
            root_from(None, Some(home)),
            Some(PathBuf::from("/home/alex/.claude"))
        );
        assert_eq!(root_from(None, None), None);
        assert_eq!(root_from(Some(OsStr::new("")), None), None);
    }

    /// `path`, written relative to the working folder.
    fn relative_to_here(path: &Path) -> PathBuf {
        let here = std::env::current_dir().unwrap();
        let ups = here.components().count() - 1;
        let mut relative: PathBuf = std::iter::repeat_n("..", ups).collect();
        relative.push(path.strip_prefix("/").unwrap());
        relative
    }

    #[test]
    fn a_relative_root_is_no_store() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        whole_session(root, "-home-alex-a", ID);
        let before = snapshot(root);
        let relative = relative_to_here(root);
        assert!(relative.is_relative() && relative.join("projects").is_dir());

        let no_store = AgentError::Io("the store could not be found".into());
        assert_eq!(paths_of(&relative, &id(ID)), Err(no_store.clone()));
        assert_eq!(remove(&relative, &id(ID), T), Err(no_store));
        assert_eq!(snapshot(root), before);
        assert_eq!(
            root_from(Some(OsStr::new("cfg")), Some(Path::new("/home/alex"))),
            None
        );
        assert_eq!(root_from(None, Some(Path::new("home/alex"))), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_change_seen_at_the_second_look_leaves_the_transcript() {
        use std::os::unix::fs::symlink;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        write(
            &root.join("file-history").join(ID).join("0a1b@v1"),
            "old text",
        );
        let history = root.join("file-history");
        std::fs::File::open(&history)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(T as u64))
            .unwrap();
        // The transcript's time is the time of `file-history`, which removing
        // `file-history/<id>` moves on: as if the session were used again meanwhile.
        let project = root.join("projects/-home-alex-a");
        std::fs::create_dir_all(&project).unwrap();
        let link = project.join(format!("{ID}.jsonl"));
        symlink(&history, &link).unwrap();

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Changed));

        assert!(!exists(&history.join(ID)));
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(history.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_store_is_an_error_not_an_empty_list() {
        use std::os::unix::fs::PermissionsExt;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        whole_session(root, "-home-alex-a", ID);
        let projects = root.join("projects");
        std::fs::set_permissions(&projects, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads a folder it has no permission for, so there is nothing to see then.
        let is_root = std::fs::read_dir(&projects).is_ok();

        let found = paths_of(root, &id(ID));

        std::fs::set_permissions(&projects, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !is_root {
            assert_eq!(
                found,
                Err(AgentError::Io("the store could not be read".into()))
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_sessions_folder_is_removed_and_its_target_stays() {
        use std::os::unix::fs::symlink;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let elsewhere = tempfile::tempdir().unwrap();
        write(&elsewhere.path().join("agents/agent-b2.jsonl"), "{}\n");
        write(&elsewhere.path().join("result.txt"), "kept");
        let paths = whole_session(root, "-home-alex-a", ID);
        symlink(
            elsewhere.path().join("agents"),
            paths[1].join("linked-agents"),
        )
        .unwrap();
        symlink(
            elsewhere.path().join("result.txt"),
            paths[1].join("result.txt"),
        )
        .unwrap();
        let outside = snapshot(elsewhere.path());

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));

        assert!(!exists(&paths[1]));
        assert_eq!(snapshot(elsewhere.path()), outside);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_session_env_folder_is_removed_and_its_target_stays() {
        use std::os::unix::fs::symlink;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let elsewhere = tempfile::tempdir().unwrap();
        write(&elsewhere.path().join("env/keep.txt"), "still here");
        transcript(root, "-home-alex-a", ID, T);
        let env = root.join("session-env").join(ID);
        std::fs::create_dir_all(root.join("session-env")).unwrap();
        symlink(elsewhere.path().join("env"), &env).unwrap();
        let outside = snapshot(elsewhere.path());

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));

        assert!(!exists(&env));
        assert_eq!(snapshot(elsewhere.path()), outside);
    }

    #[cfg(unix)]
    #[test]
    fn a_session_in_a_linked_project_folder_is_removed_where_the_link_points() {
        use std::os::unix::fs::symlink;
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("-home-alex-a");
        let real = target.join(format!("{ID}.jsonl"));
        write(&real, "{\"type\":\"user\"}\n");
        set_modified(&real, T);
        write(&target.join("notes.txt"), "kept");
        std::fs::create_dir_all(root.join("projects")).unwrap();
        let link = root.join("projects/-home-alex-a");
        symlink(&target, &link).unwrap();

        assert_eq!(remove(root, &id(ID), T), Ok(Removal::Removed));

        assert!(!exists(&real));
        assert_eq!(
            std::fs::read_to_string(target.join("notes.txt")).unwrap(),
            "kept"
        );
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
