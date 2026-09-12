//! Hand-built repository layouts chosen to break naive ignore handling, each synced through the
//! real pipeline and compared with the `check-ignore` oracle.

mod common;

use std::os::unix::fs::symlink;
use std::path::Path;

use common::{check, commit_all, git, repo, write};
use tempfile::{tempdir, TempDir};

fn workspace() -> (TempDir, std::path::PathBuf) {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("root");
    (tmp, root)
}

fn add_submodule(root: &Path, path: &str, upstream: &Path) {
    git(
        root,
        &[
            "submodule",
            "add",
            "--quiet",
            upstream.to_str().unwrap(),
            path,
        ],
    );
    commit_all(root);
}

#[test]
fn tracked_file_inside_ignored_directory() {
    let (tmp, root) = workspace();
    repo(&root, "build/\n");
    write(root.join("build/keep"), "");
    write(root.join("build/x"), "");
    write(root.join("build/inner/y"), "");
    git(&root, &["add", "--force", "build/keep"]);
    commit_all(&root);

    let synced = check(&root, tmp.path(), "tracked file inside ignored directory");
    assert_eq!(synced, [".gitignore", "build/keep"]);
}

#[test]
fn negation_across_levels_and_under_ignored_directory() {
    let (tmp, root) = workspace();
    repo(&root, "*.log\nlogs/\n");
    write(root.join("sub/.gitignore"), "!keep.log\n");
    write(root.join("sub/deep/.gitignore"), "keep.log\n");
    write(root.join("logs/.gitignore"), "!important\n");
    for file in [
        "a.log",
        "sub/keep.log",
        "sub/other.log",
        "sub/deep/keep.log",
        "logs/important",
        "logs/other",
    ] {
        write(root.join(file), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "negation across levels");
    assert_eq!(
        synced,
        [
            ".gitignore",
            "sub/.gitignore",
            "sub/deep/.gitignore",
            "sub/keep.log"
        ]
    );
}

#[test]
fn embedded_repository_without_gitmodules() {
    let (tmp, root) = workspace();
    repo(&root, "*.tmp\n");
    let embedded = root.join("embedded");
    repo(&embedded, "etarget/\n");
    write(embedded.join("etarget/e"), "");
    write(embedded.join("e.tmp"), "");
    write(embedded.join("kept"), "");
    commit_all(&embedded);
    commit_all(&root);

    let synced = check(&root, tmp.path(), "embedded repository without .gitmodules");
    assert!(synced.contains(&"embedded/e.tmp".to_string()));
    assert!(!synced.iter().any(|f| f.starts_with("embedded/etarget/")));
}

#[test]
fn nested_repository_inside_untracked_directory() {
    let (tmp, root) = workspace();
    repo(&root, "*.log\n");
    write(root.join("tracked"), "");
    commit_all(&root);
    let nested = root.join("untracked/deeper/nested");
    repo(&nested, "ntarget/\n");
    write(nested.join("ntarget/z"), "");
    write(nested.join("n.log"), "");
    commit_all(&nested);
    write(root.join("untracked/u.log"), "");
    write(root.join("untracked/u"), "");

    let synced = check(
        &root,
        tmp.path(),
        "nested repository inside untracked directory",
    );
    assert!(synced.contains(&"untracked/deeper/nested/n.log".to_string()));
    assert!(!synced.contains(&"untracked/u.log".to_string()));
}

#[test]
fn nested_repository_names_with_wildcards() {
    let (tmp, root) = workspace();
    repo(&root, "");
    commit_all(&root);
    for name in ["we*ird", "q?", "br[k]", "sp ace"] {
        let nested = root.join(name);
        repo(&nested, "ntarget/\n*.log\n");
        write(nested.join("ntarget/z"), "");
        write(nested.join("n.log"), "");
        write(nested.join("kept"), "");
        commit_all(&nested);
    }
    write(root.join("weird/kept.log"), "");
    write(root.join("q1/kept.log"), "");

    let synced = check(&root, tmp.path(), "nested repository names with wildcards");
    assert!(synced.contains(&"weird/kept.log".to_string()));
    assert!(synced.contains(&"q1/kept.log".to_string()));
    assert!(!synced.contains(&"we*ird/n.log".to_string()));
}

#[test]
fn sync_root_is_a_subdirectory() {
    let (tmp, root) = workspace();
    repo(&root, "*.log\n!keep.log\n/toplevel-only\n");
    write(root.join("sub/.gitignore"), "local/\n");
    for file in [
        "sub/a.log",
        "sub/keep.log",
        "sub/toplevel-only",
        "sub/local/x",
        "sub/deep/b.log",
        "sub/deep/keep",
    ] {
        write(root.join(file), "");
    }
    commit_all(&root);

    let synced = check(&root.join("sub"), tmp.path(), "sync root is a subdirectory");
    assert_eq!(
        synced,
        [".gitignore", "deep/keep", "keep.log", "toplevel-only"]
    );
}

#[test]
fn gitignore_ignoring_itself() {
    let (tmp, root) = workspace();
    repo(&root, ".gitignore\n*.log\n");
    write(root.join("a.log"), "");
    write(root.join("kept"), "");
    commit_all(&root);

    let synced = check(&root, tmp.path(), ".gitignore ignoring itself");
    assert_eq!(synced, ["kept"]);
}

#[test]
fn symlinks_are_files() {
    let (tmp, root) = workspace();
    repo(&root, "build/\n*.log\nlink-ignored\n");
    write(root.join("build/x"), "");
    write(root.join("real/f"), "");
    symlink("build", root.join("to-ignored-dir")).unwrap();
    symlink("real", root.join("to-dir")).unwrap();
    symlink("missing", root.join("dangling")).unwrap();
    symlink("real/f", root.join("link-ignored")).unwrap();
    symlink("real/f", root.join("link.log")).unwrap();
    commit_all(&root);

    let synced = check(&root, tmp.path(), "symlinks are files");
    assert!(synced.contains(&"to-ignored-dir".to_string()));
    assert!(synced.contains(&"dangling".to_string()));
    assert!(!synced.contains(&"link-ignored".to_string()));
    assert!(!synced.contains(&"link.log".to_string()));
}

#[test]
fn special_pattern_syntax() {
    let (tmp, root) = workspace();
    repo(
        &root,
        "# comment\r\n\r\ntrail\\ \r\n\\#hash\r\n\\!bang\r\n[!a]x\r\n[a-c]y\r\n**/deep\r\ndeep2/**\r\nmid/**/end\r\n*.LOG\r\n",
    );
    for file in [
        "trail ",
        "trail",
        "#hash",
        "!bang",
        "bx",
        "ax",
        "by",
        "dy",
        "deep",
        "a/deep",
        "a/b/deep",
        "deep2/anything/here",
        "mid/end",
        "mid/x/end",
        "mid/x/y/end",
        "upper.LOG",
        "lower.log",
    ] {
        write(root.join(file), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "special pattern syntax");
    assert_eq!(synced, [".gitignore", "ax", "dy", "lower.log", "trail"]);
}

#[test]
fn ignored_directory_holding_nested_repository_and_tracked_file() {
    let (tmp, root) = workspace();
    repo(&root, "build/\n");
    write(root.join("build/keep"), "");
    write(root.join("build/x"), "");
    git(&root, &["add", "--force", "build/keep"]);
    commit_all(&root);
    let nested = root.join("build/nested");
    repo(&nested, "");
    write(nested.join("n"), "");
    commit_all(&nested);

    let synced = check(
        &root,
        tmp.path(),
        "ignored directory holding nested repository",
    );
    assert_eq!(synced, [".gitignore", "build/keep"]);
}

#[test]
fn nested_repository_with_info_exclude() {
    let (tmp, root) = workspace();
    repo(&root, "");
    commit_all(&root);
    let nested = root.join("nested");
    repo(&nested, "");
    write(nested.join(".git/info/exclude"), "private\n");
    write(nested.join("private"), "");
    write(nested.join("public"), "");
    commit_all(&nested);

    let synced = check(&root, tmp.path(), "nested repository with info/exclude");
    assert_eq!(synced, [".gitignore", "nested/.gitignore", "nested/public"]);
}

#[test]
fn submodule_inside_nested_repository_and_uninitialized_submodule() {
    let (tmp, root) = workspace();
    let upstream = tmp.path().join("upstream");
    repo(&upstream, "smbuild/\n");
    write(upstream.join("s"), "");
    commit_all(&upstream);

    repo(&root, "");
    commit_all(&root);
    let nested = root.join("nested");
    repo(&nested, "");
    commit_all(&nested);
    add_submodule(&nested, "sm", &upstream);
    write(nested.join("sm/smbuild/q"), "");

    add_submodule(&root, "empty", &upstream);
    git(
        &root,
        &["submodule", "deinit", "--force", "--quiet", "empty"],
    );

    let synced = check(&root, tmp.path(), "submodule inside nested repository");
    assert!(synced.contains(&"nested/sm/s".to_string()));
    assert!(!synced.contains(&"nested/sm/smbuild/q".to_string()));
}

#[test]
fn thousands_of_scattered_ignored_files() {
    let (tmp, root) = workspace();
    repo(&root, "*.o\n");
    for i in 0..4000 {
        write(root.join(format!("dir_{i:04}/keep")), "");
        write(root.join(format!("dir_{i:04}/x.o")), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "thousands of scattered ignored files");
    assert_eq!(synced.len(), 4001);
}

#[test]
fn names_with_leading_punctuation() {
    let (tmp, root) = workspace();
    repo(&root, "-dash\n+plus\n\\!bang\n;semi\n\\#hash\n");
    for file in ["-dash", "+plus", "!bang", ";semi", "#hash", "-kept"] {
        write(root.join(file), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "names with leading punctuation");
    assert_eq!(synced, ["-kept", ".gitignore"]);
}

#[test]
fn directory_only_patterns() {
    let (tmp, root) = workspace();
    repo(&root, "foo/\nbar\n");
    for file in ["a/foo/x", "b/foo", "c/bar/y", "d/bar"] {
        write(root.join(file), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "directory-only patterns");
    assert_eq!(synced, [".gitignore", "b/foo"]);
}

#[test]
fn ignored_sync_root_holding_a_tracked_file() {
    let (tmp, root) = workspace();
    repo(&root, "data/\n");
    write(root.join("data/keep"), "");
    write(root.join("data/x"), "");
    git(&root, &["add", "--force", "data/keep"]);
    commit_all(&root);

    let synced = check(
        &root.join("data"),
        tmp.path(),
        "ignored sync root holding a tracked file",
    );
    assert_eq!(synced, ["keep"]);
}

#[test]
fn deep_gitignore_chain() {
    let (tmp, root) = workspace();
    repo(&root, "*.log\n");
    write(root.join("l1/.gitignore"), "!*.log\n");
    write(root.join("l1/l2/.gitignore"), "*.log\n");
    write(root.join("l1/l2/l3/.gitignore"), "!keep.log\n");
    write(root.join("l1/l2/l3/l4/.gitignore"), "keep.log\n");
    for dir in [
        "",
        "l1/",
        "l1/l2/",
        "l1/l2/l3/",
        "l1/l2/l3/l4/",
        "l1/l2/l3/l4/l5/",
    ] {
        write(root.join(format!("{dir}keep.log")), "");
        write(root.join(format!("{dir}other.log")), "");
    }
    commit_all(&root);

    let synced = check(&root, tmp.path(), "deep .gitignore chain");
    assert!(synced.contains(&"l1/keep.log".to_string()));
    assert!(synced.contains(&"l1/other.log".to_string()));
    assert!(synced.contains(&"l1/l2/l3/keep.log".to_string()));
    assert!(!synced.contains(&"l1/l2/l3/other.log".to_string()));
    assert!(!synced.contains(&"l1/l2/l3/l4/keep.log".to_string()));
    assert!(!synced.contains(&"l1/l2/l3/l4/l5/keep.log".to_string()));
}
