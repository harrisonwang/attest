//! 各模块测试共用的夹具。

use std::{fs, path::Path, process::Command};

pub(crate) fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"fixture","scripts":{"test":"echo ok"}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("AGENTS.md"),
        "Run `npm run test` and `npm run missing`.",
    )
    .unwrap();
    directory
}

pub(crate) fn init_git(directory: &Path) {
    for args in [
        &["init"][..],
        &["config", "user.email", "attest@example.invalid"],
        &["config", "user.name", "attest test"],
        &["add", "."],
        &["commit", "-m", "initial"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(directory)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
