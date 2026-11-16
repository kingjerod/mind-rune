//! The Linux desktop entry: `scripts/install-desktop-entry.sh` puts the entry and the icons
//! where a desktop looks for them, and takes them away again.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the script with `data` as the user's data folder; its exit status and standard error.
fn script(data: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("sh")
        .arg(repo().join("scripts/install-desktop-entry.sh"))
        .args(args)
        .env("XDG_DATA_HOME", data)
        .output()
        .expect("sh runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A file that can be run, in a folder whose name needs quoting in an `Exec` line.
fn binary(dir: &Path) -> PathBuf {
    let folder = dir.join("my builds");
    fs::create_dir(&folder).unwrap();
    let path = folder.join("mindrune");
    fs::write(&path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn installing_puts_the_entry_and_the_icons_where_a_desktop_looks() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let binary = binary(dir.path());

    let (ok, err) = script(&data, &[binary.to_str().unwrap()]);
    assert!(ok, "{err}");

    let entry = fs::read_to_string(data.join("applications/mindrune.desktop")).unwrap();
    let exec = format!("Exec=\"{}\"", binary.display());
    assert!(entry.lines().any(|line| line == exec), "{entry}");
    assert!(entry.lines().any(|line| line == "Icon=mindrune"), "{entry}");
    for size in [16, 32, 48, 64, 128, 256, 512] {
        let installed = data.join(format!("icons/hicolor/{size}x{size}/apps/mindrune.png"));
        let ours = repo().join(format!("assets/icon/mindrune-{size}.png"));
        assert_eq!(
            fs::read(&installed).ok(),
            Some(fs::read(ours).unwrap()),
            "{}",
            installed.display()
        );
    }
    assert!(
        data.join("icons/hicolor/scalable/apps/mindrune.svg")
            .is_file()
    );
}

#[test]
fn uninstalling_takes_away_what_installing_put_there_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let binary = binary(dir.path());
    let other = data.join("applications/other.desktop");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, "[Desktop Entry]\n").unwrap();
    assert!(script(&data, &[binary.to_str().unwrap()]).0);

    let (ok, err) = script(&data, &["--uninstall"]);
    assert!(ok, "{err}");

    assert!(!data.join("applications/mindrune.desktop").exists());
    assert!(!data.join("icons/hicolor/48x48/apps/mindrune.png").exists());
    assert!(
        !data
            .join("icons/hicolor/scalable/apps/mindrune.svg")
            .exists()
    );
    assert!(other.is_file());
}

#[test]
fn a_binary_that_is_not_there_installs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");

    let (ok, err) = script(&data, &["/no/such/mindrune"]);

    assert!(!ok);
    assert!(err.contains("/no/such/mindrune"), "{err}");
    assert!(!data.join("applications/mindrune.desktop").exists());
}
