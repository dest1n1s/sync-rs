#![allow(dead_code)]

pub mod oracle;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use sync_rs::ignore::git_ignored_paths;
use tempfile::tempdir;

pub fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=sync-rs",
            "-c",
            "user.email=sync-rs@test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "protocol.file.allow=always",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "advice.addEmbeddedRepo=false",
        ])
        .args(args)
        .output()
        .expect("git is installed");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn write(path: impl AsRef<Path>, contents: &str) {
    fs::create_dir_all(path.as_ref().parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

pub fn repo(dir: &Path, gitignore: &str) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
    write(dir.join(".gitignore"), gitignore);
}

pub fn commit_all(dir: &Path) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["commit", "--quiet", "--allow-empty", "-m", "snapshot"],
    );
}

/// Files under `dir` as relative paths, skipping git metadata.
pub fn tree(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap() == ".git" {
                continue;
            }
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap();
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Mirror `src` into `dst` with the flags `sync_directory` uses for streamed rules, minus the
/// progress output.
pub fn rsync_excluding(src: &Path, dst: &Path, patterns: &[Vec<u8>]) {
    let mut child = Command::new("rsync")
        .args(["-a", "--quiet", "--from0", "--filter", "merge -"])
        .arg(format!("{}/", src.display()))
        .arg(dst)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for pattern in patterns {
            stdin.write_all(b"- ").unwrap();
            stdin.write_all(pattern).unwrap();
            stdin.write_all(b"\0").unwrap();
        }
    }
    assert!(child.wait().unwrap().success(), "rsync failed");
}

/// Sync `sync_root` through the real exclusion pipeline and assert the result matches the
/// oracle; returns the synced tree. `context` is shown on mismatch.
pub fn check(sync_root: &Path, stop: &Path, context: &str) -> Vec<String> {
    let excludes = git_ignored_paths(sync_root)
        .unwrap()
        .expect("git is installed");
    let dst = tempdir().unwrap();
    rsync_excluding(sync_root, dst.path(), &excludes);
    let actual = tree(dst.path());
    let expected = oracle::expected_files(sync_root, stop);
    let shown: Vec<String> = excludes
        .iter()
        .map(|pattern| String::from_utf8_lossy(pattern).into_owned())
        .collect();
    assert_eq!(actual, expected, "{context}\nexcludes: {shown:?}");
    actual
}
