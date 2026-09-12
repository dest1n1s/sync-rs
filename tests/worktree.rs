mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{commit_all, git, repo, write};
use sync_rs::ignore::git_ignored_paths;
use sync_rs::worktree::{suffix_for, Location};
use tempfile::{tempdir, TempDir};

/// A committed repository at `<tmp>/proj` with a `sub/` directory, on branch `main`.
fn project() -> (TempDir, PathBuf) {
    let tmp = tempdir().unwrap();
    let proj = tmp.path().canonicalize().unwrap().join("proj");
    repo(&proj, "*.log\n");
    write(proj.join("sub/file"), "");
    write(proj.join("top"), "");
    commit_all(&proj);
    (tmp, proj)
}

fn worktrees(repo: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn main_worktree_is_plain() {
    let (_tmp, proj) = project();
    let location = Location::discover(&proj.join("sub")).unwrap();
    assert_eq!(location.config_dir(), proj.join("sub"));
    let target = location.target(None).unwrap();
    assert_eq!(target.source, proj.join("sub"));
    assert_eq!(target.suffix, None);
}

#[test]
fn linked_worktree_shares_config_and_names_its_branch() {
    let (tmp, proj) = project();
    let linked = tmp.path().canonicalize().unwrap().join("proj-feature");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );

    let location = Location::discover(&linked.join("sub")).unwrap();
    assert_eq!(location.config_dir(), proj.join("sub"));
    let target = location.target(None).unwrap();
    assert_eq!(target.source, linked.join("sub"));
    assert_eq!(target.suffix.as_deref(), Some("feature"));
}

#[test]
fn detached_worktree_is_named_after_its_directory() {
    let (tmp, proj) = project();
    let scratch = tmp.path().canonicalize().unwrap().join("scratch");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            scratch.to_str().unwrap(),
            "HEAD",
        ],
    );

    let target = Location::discover(&scratch).unwrap().target(None).unwrap();
    assert_eq!(target.suffix.as_deref(), Some("scratch"));
}

#[test]
fn branch_uses_its_existing_worktree() {
    let (tmp, proj) = project();
    let linked = tmp.path().canonicalize().unwrap().join("proj-feature");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );

    let from_main = Location::discover(&proj.join("sub")).unwrap();
    let target = from_main.target(Some("feature")).unwrap();
    assert_eq!(target.source, linked.join("sub"));
    assert_eq!(target.suffix.as_deref(), Some("feature"));

    let from_linked = Location::discover(&linked).unwrap();
    let target = from_linked.target(Some("main")).unwrap();
    assert_eq!(target.source, proj);
    assert_eq!(target.suffix, None);
}

#[test]
fn branch_without_worktree_is_checked_out_temporarily() {
    let (_tmp, proj) = project();
    git(&proj, &["branch", "--quiet", "feat/x"]);
    git(&proj, &["checkout", "--quiet", "feat/x"]);
    write(proj.join("only-on-branch"), "");
    commit_all(&proj);
    git(&proj, &["checkout", "--quiet", "main"]);
    assert!(!proj.join("only-on-branch").exists());

    let location = Location::discover(&proj).unwrap();
    let target = location.target(Some("feat/x")).unwrap();
    assert_eq!(target.suffix.as_deref(), Some("feat-x"));
    assert!(target.source.join("only-on-branch").exists());
    assert!(target.source.join(".git").is_file());
    let listing = worktrees(&proj);
    assert!(
        listing.contains(target.source.to_str().unwrap()),
        "{listing}\nnot listing {}",
        target.source.display()
    );
    assert_eq!(
        git_ignored_paths(&target.source).unwrap().unwrap(),
        Vec::<Vec<u8>>::new()
    );

    let source = target.source.clone();
    drop(target);
    assert!(!source.exists());
    assert!(!worktrees(&proj).contains(source.to_str().unwrap()));
}

#[test]
fn branch_needs_a_repository_and_a_known_ref() {
    let tmp = tempdir().unwrap();
    let plain = tmp.path().join("plain");
    write(plain.join("file"), "");
    assert!(Location::discover(&plain)
        .unwrap()
        .target(Some("main"))
        .is_err());

    let (_tmp, proj) = project();
    assert!(Location::discover(&proj)
        .unwrap()
        .target(Some("no-such-branch"))
        .is_err());
}

#[test]
fn branch_missing_the_subdirectory_is_an_error() {
    let (_tmp, proj) = project();
    git(&proj, &["checkout", "--quiet", "-b", "bare"]);
    git(&proj, &["rm", "--quiet", "-r", "sub"]);
    commit_all(&proj);
    git(&proj, &["checkout", "--quiet", "main"]);

    let location = Location::discover(&proj.join("sub")).unwrap();
    assert!(location.target(Some("bare")).is_err());
}

#[test]
fn live_suffixes_cover_worktrees_and_refs() {
    let (tmp, proj) = project();
    let linked = tmp.path().canonicalize().unwrap().join("proj-feature");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );
    let scratch = tmp.path().canonicalize().unwrap().join("scratch");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            scratch.to_str().unwrap(),
            "HEAD",
        ],
    );
    git(&proj, &["branch", "--quiet", "feat/x"]);
    git(&proj, &["tag", "v1"]);

    let live = Location::discover(&proj)
        .unwrap()
        .live_suffixes()
        .unwrap()
        .unwrap();
    for suffix in ["feature", "scratch", "feat-x", "v1", "main"] {
        assert!(live.contains(suffix), "missing {suffix} in {live:?}");
    }
    assert!(!live.contains("gone"));
    assert_eq!(suffix_for("feat/x"), "feat-x");

    let plain = tmp.path().join("plain");
    write(plain.join("file"), "");
    assert!(Location::discover(&plain)
        .unwrap()
        .live_suffixes()
        .unwrap()
        .is_none());
}
