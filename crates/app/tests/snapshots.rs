//! A screenshot of every screen, in both themes, made by the built binary from the demo's
//! scripted scenes. Each run is headless (`--snap` opens no window) and uses `--demo`, so it
//! never reaches the real agent. The images land in `target/snap/final` for a person to look at.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The scenes that make up the window's screens: one session's, in the order it meets them,
/// then those with many sessions, the sidebar's menus, a stored session's, tiles side by side,
/// the right panel beside them, the chips of an input box, the question whether to quit, a
/// picture open large, the colours of sessions that are ready or need the user, the Deleted
/// view with its questions, and a task list open and folded and a choice box. Each runs in the
/// binary's own window size unless a size is given, as `--size` takes it.
const SCENES: &[(&str, Option<&str>)] = &[
    ("checking", None),
    ("signed-out", None),
    ("not-installed", None),
    ("empty", None),
    ("effort", None),
    ("limit", None),
    ("conversation", None),
    ("streaming", None),
    ("tools", None),
    ("working", None),
    ("ended", None),
    ("permission", None),
    ("permission-edit", None),
    ("question", None),
    ("plan", None),
    ("sessions", None),
    ("group-menu", None),
    ("folders", None),
    ("empty-start", None),
    ("resting", None),
    ("history-loading", None),
    ("history-failed", None),
    ("elsewhere", None),
    ("tiles", Some(WIDE)),
    ("tiles-needs-you", Some(WIDE)),
    ("tiles-drag", Some(WIDE)),
    ("tiles-stack-drag", Some(WIDE)),
    ("tiles-move", Some(WIDE)),
    ("tiles-stacked", Some(WIDE)),
    ("tile-rename", None),
    ("panel", Some(WIDE)),
    ("panel-remote", Some(WIDE)),
    ("panel-refused", Some(WIDE)),
    ("chips", None),
    ("chips-problem", None),
    ("quit-ask", None),
    ("viewer", None),
    ("viewer-failed", None),
    ("colours", Some(WIDE)),
    ("deleted", None),
    ("deleted-empty", None),
    ("delete-group-ask", None),
    ("delete-all-ask", None),
    ("tasks", None),
    // The binary's own 1,200 by 800 is the size the scene's column of two is made for: its
    // upper tile is short enough that the list starts folded.
    ("tasks-folded", None),
    ("choices", None),
];

/// The scenes whose part arrives after the first frame, with that part's key before its
/// session's number (`mr-tasks-1`): a task list and a choice box are drawn from the second
/// frame of the app's life and slide and fade in. They are taken at [`SETTLED_AT`], and the
/// part must be drawn there at rest.
const ARRIVING: &[(&str, &str)] = &[
    ("tasks", "mr-tasks-"),
    ("tasks-folded", "mr-tasks-"),
    ("choices", "mr-choices-"),
];

/// When a scene of [`ARRIVING`] is taken, in seconds, as `--at` takes it: well after its part
/// has settled, about 0.3 s after it first shows.
const SETTLED_AT: &str = "1";

/// A window wide enough for three tiles of more than 420 points each beside the sidebar. The
/// `tiles-drag`, `tiles-stack-drag` and `tiles-move` scenes place their line or outline for
/// this size: see `WIDE_ROW` in `src/demo.rs`.
const WIDE: &str = "1800x900";

/// An image smaller than this is taken to be blank.
const MIN_IMAGE_BYTES: u64 = 2_000;

/// What Rune prints on stderr for a mistake in the view rather than a slow frame.
const VIEW_MISTAKES: &[&str] = &["duplicate sibling key", "replaced its root's key"];

#[test]
fn every_screen_in_the_dark_theme() {
    snap_every_scene("dark");
}

#[test]
fn every_screen_in_the_light_theme() {
    snap_every_scene("light");
}

fn snap_every_scene(theme: &str) {
    let out = snap_dir();
    std::fs::create_dir_all(&out).expect("the snapshot folder can be made");
    // Rune's own files (settings, caches) go to a folder of this run's, never the user's.
    let dirs_root = tempfile::tempdir().expect("a temporary folder");
    for &(scene, size) in SCENES {
        snap(scene, size, theme, &out, dirs_root.path());
    }
}

/// `target/snap/final`, next to the binary's own build folder.
fn snap_dir() -> PathBuf {
    let binary = Path::new(env!("CARGO_BIN_EXE_mindrune"));
    let target = binary
        .parent()
        .and_then(Path::parent)
        .expect("the binary sits in target/<profile>");
    target.join("snap").join("final")
}

fn snap(scene: &str, size: Option<&str>, theme: &str, out: &Path, dirs_root: &Path) {
    let image = out.join(format!("{scene}-{theme}.png"));
    let tree = out.join(format!("{scene}-{theme}.json"));
    let _ = std::fs::remove_file(&image);
    let _ = std::fs::remove_file(&tree);
    let mut command = Command::new(env!("CARGO_BIN_EXE_mindrune"));
    command
        .args(["--demo", "--scene", scene, "--theme", theme, "--snap"])
        .arg(&image)
        .arg("--tree")
        .arg(&tree)
        .env("RUNE_DIRS_ROOT", dirs_root);
    if let Some(size) = size {
        command.args(["--size", size]);
    }
    let arriving = ARRIVING
        .iter()
        .find(|(name, _)| *name == scene)
        .map(|&(_, part)| part);
    if arriving.is_some() {
        command.args(["--at", SETTLED_AT]);
    }
    let run = command
        .output()
        .unwrap_or_else(|e| panic!("{scene} ({theme}): the binary did not start: {e}"));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "{scene} ({theme}): exited with {}; stderr:\n{stderr}",
        run.status
    );
    for mistake in VIEW_MISTAKES {
        assert!(
            !stderr.contains(mistake),
            "{scene} ({theme}): Rune warned about the view; stderr:\n{stderr}"
        );
    }

    let bytes = std::fs::metadata(&image)
        .unwrap_or_else(|e| panic!("{scene} ({theme}): no image at {}: {e}", image.display()))
        .len();
    assert!(
        bytes > MIN_IMAGE_BYTES,
        "{scene} ({theme}): the image has only {bytes} bytes; stderr:\n{stderr}"
    );

    let text = std::fs::read_to_string(&tree)
        .unwrap_or_else(|e| panic!("{scene} ({theme}): no tree at {}: {e}", tree.display()));
    let tree: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{scene} ({theme}): the tree is not JSON: {e}"));
    let warnings = tree["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("{scene} ({theme}): the tree has no warnings list"));
    assert!(
        warnings.is_empty(),
        "{scene} ({theme}): the tree has warnings: {warnings:?}"
    );
    let elements = tree["elements"]
        .as_array()
        .unwrap_or_else(|| panic!("{scene} ({theme}): the tree has no elements list"));
    assert!(
        elements.iter().any(|e| e["key"] == "mr-root"),
        "{scene} ({theme}): no element keyed mr-root"
    );
    if let Some(part) = arriving {
        let at_rest = elements.iter().any(|e| {
            e["key"]
                .as_str()
                .and_then(|key| key.strip_prefix(part))
                .is_some_and(|n| n.parse::<u64>().is_ok())
                && e["opacity"] == 1.0
                && e.get("moving").is_none_or(|moving| moving != true)
        });
        assert!(
            at_rest,
            "{scene} ({theme}): no element keyed {part}N drawn at rest"
        );
    }
}
