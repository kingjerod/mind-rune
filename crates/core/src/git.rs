//! The git branch of a folder, read from the repository's own files: no process is started.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

/// The most folders looked in for `.git`, the folder asked about included.
const CLIMB: usize = 64;
/// The most bytes read of any one file.
const READ_LIMIT: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Branch {
    Named(String),
    /// The first seven characters of the commit a detached head is at.
    Detached(String),
}

/// The branch of the repository `folder` is in, read from its files. None outside a
/// repository and for anything that cannot be read.
pub fn branch(folder: &Path) -> Option<Branch> {
    if !folder.is_dir() {
        return None;
    }
    let dot_git = folder
        .ancestors()
        .take(CLIMB)
        .map(|above| above.join(".git"))
        .find(|dot_git| fs::symlink_metadata(dot_git).is_ok())?;
    let git = if dot_git.is_dir() {
        dot_git
    } else {
        // A worktree's or a submodule's: it names the folder that holds its own HEAD.
        let line = first_line(&dot_git)?;
        let path = line.strip_prefix("gitdir:")?.trim();
        if path.is_empty() {
            return None;
        }
        dot_git.parent()?.join(path)
    };
    head(&first_line(&git.join("HEAD"))?)
}

/// What a `HEAD` line says.
fn head(line: &str) -> Option<Branch> {
    if let Some(reference) = line.strip_prefix("ref:") {
        let name = reference.trim().strip_prefix("refs/heads/")?;
        return (!name.is_empty()).then(|| Branch::Named(name.to_string()));
    }
    let commit = matches!(line.len(), 40 | 64) && line.bytes().all(|b| b.is_ascii_hexdigit());
    commit.then(|| Branch::Detached(line[..7].to_string()))
}

/// The first line of the file at `path`, without its line ending. None when it cannot be read,
/// is not text, or goes on past what is read.
fn first_line(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(READ_LIMIT)
        .read_to_end(&mut bytes)
        .ok()?;
    let end = match bytes.iter().position(|&b| b == b'\n') {
        Some(end) => end,
        None if bytes.len() as u64 == READ_LIMIT => return None,
        None => bytes.len(),
    };
    bytes.truncate(end);
    let line = String::from_utf8(bytes).ok()?;
    Some(line.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A repository at `root`, its head holding `head`.
    fn repository(root: &Path, head: &str) -> PathBuf {
        let git = root.join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("HEAD"), head).unwrap();
        git
    }

    fn named(name: &str) -> Option<Branch> {
        Some(Branch::Named(name.into()))
    }

    #[test]
    fn a_repository_gives_its_branch() {
        let dir = tempfile::tempdir().unwrap();
        let git = repository(dir.path(), "ref: refs/heads/main\n");
        assert_eq!(branch(dir.path()), named("main"));
        let deep = dir.path().join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(branch(&deep), named("main"));
        fs::write(git.join("HEAD"), "ref: refs/heads/feature/panel\n").unwrap();
        assert_eq!(branch(&deep), named("feature/panel"));
    }

    #[test]
    fn a_worktree_gives_its_own_branch() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("notes");
        let git = repository(&main, "ref: refs/heads/main\n");
        for (name, branch_name) in [("one", "first"), ("two", "second")] {
            let own = git.join("worktrees").join(name);
            fs::create_dir_all(&own).unwrap();
            fs::write(own.join("HEAD"), format!("ref: refs/heads/{branch_name}\n")).unwrap();
            fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        let absolute = git.join("worktrees/one");
        fs::write(
            dir.path().join("one/.git"),
            format!("gitdir: {}\n", absolute.display()),
        )
        .unwrap();
        fs::write(
            dir.path().join("two/.git"),
            "gitdir: ../notes/.git/worktrees/two\n",
        )
        .unwrap();
        assert_eq!(branch(&dir.path().join("one")), named("first"));
        assert_eq!(branch(&dir.path().join("two")), named("second"));
        assert_eq!(branch(&main), named("main"));
    }

    #[test]
    fn a_detached_head_gives_seven_characters() {
        let dir = tempfile::tempdir().unwrap();
        let git = repository(dir.path(), &format!("{COMMIT}\n"));
        assert_eq!(branch(dir.path()), Some(Branch::Detached("0123456".into())));
        let longer = format!("fedcba98{COMMIT}{}", &COMMIT[..16]);
        fs::write(git.join("HEAD"), longer).unwrap();
        assert_eq!(branch(dir.path()), Some(Branch::Detached("fedcba9".into())));
    }

    #[test]
    fn outside_a_repository_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("plain");
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(branch(&folder), None);
        // A repository further up than the climb reaches is out of reach.
        repository(dir.path(), "ref: refs/heads/main\n");
        let reached = (1..CLIMB).fold(dir.path().to_path_buf(), |path, _| path.join("d"));
        fs::create_dir_all(reached.join("d")).unwrap();
        assert_eq!(branch(&reached), named("main"));
        assert_eq!(branch(&reached.join("d")), None);
    }

    #[test]
    fn a_head_that_cannot_be_read_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let git = repository(dir.path(), "");
        assert_eq!(branch(dir.path()), None, "empty");
        let long = format!("ref: refs/heads/{}", "a".repeat(READ_LIMIT as usize));
        for garbage in [
            "not a head\n",
            "ref: refs/heads/\n",
            "ref: refs/remotes/origin/main\n",
            "0123456\n",
            &format!("{COMMIT}0\n"),
            &format!("{}g\n", &COMMIT[..39]),
            &long,
        ] {
            fs::write(git.join("HEAD"), garbage).unwrap();
            assert_eq!(branch(dir.path()), None, "{garbage:.40}");
        }
        fs::remove_file(git.join("HEAD")).unwrap();
        fs::create_dir(git.join("HEAD")).unwrap();
        assert_eq!(branch(dir.path()), None, "a folder named HEAD");

        let worktree = dir.path().join("elsewhere");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join(".git"), "gitdir: ../nowhere\n").unwrap();
        assert_eq!(branch(&worktree), None, "a gitdir that points nowhere");
        fs::write(worktree.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        for garbage in ["not a gitdir\n", "gitdir:\n", "gitdir: \n"] {
            fs::write(worktree.join(".git"), garbage).unwrap();
            assert_eq!(branch(&worktree), None, "{garbage}");
        }
    }

    #[test]
    fn a_folder_that_is_gone_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        repository(dir.path(), "ref: refs/heads/main\n");
        let gone = dir.path().join("gone");
        fs::create_dir_all(&gone).unwrap();
        assert_eq!(branch(&gone), named("main"));
        fs::remove_dir(&gone).unwrap();
        assert_eq!(branch(&gone), None);
    }
}
