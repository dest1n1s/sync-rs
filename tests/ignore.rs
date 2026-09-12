mod common;

use std::path::Path;

use common::{commit_all, git, repo, tree, write};
use sync_rs::ignore::git_ignored_paths;
use sync_rs::sync::{sync_directory, Rules};
use tempfile::{tempdir, TempDir};

fn excludes(root: &Path) -> Vec<String> {
    let mut patterns: Vec<String> = git_ignored_paths(root)
        .unwrap()
        .expect("git is installed")
        .iter()
        .map(|pattern| String::from_utf8_lossy(pattern).into_owned())
        .collect();
    patterns.sort();
    patterns
}

fn workspace() -> TempDir {
    tempdir().unwrap()
}

#[test]
fn mirrors_git_ignore_rules() {
    let tmp = workspace();
    let root = tmp.path().join("repo");
    repo(&root, "*.log\n!keep.log\nbuild/\n/anchored\n");
    write(root.join("sub/.gitignore"), "deepignored\n");
    write(root.join("objs/.gitignore"), "*.o\n");
    write(root.join(".git/info/exclude"), "excluded-by-info\n");
    write(tmp.path().join("global-excludes"), "excluded-globally\n");
    git(
        &root,
        &[
            "config",
            "core.excludesFile",
            tmp.path().join("global-excludes").to_str().unwrap(),
        ],
    );
    for file in [
        "a.log",
        "keep.log",
        "build/x",
        "anchored/y",
        "sub/anchored/z",
        "sub/deep/deepignored",
        "sub/deep/y.log",
        "sub/keep.txt",
        "tracked.log",
        "excluded-by-info",
        "excluded-globally",
        "objs/a.o",
        "objs/b.o",
    ] {
        write(root.join(file), "");
    }
    git(&root, &["add", "--force", "tracked.log"]);
    commit_all(&root);

    assert_eq!(
        excludes(&root),
        [
            "/a.log",
            "/anchored/",
            "/build/",
            "/excluded-by-info",
            "/excluded-globally",
            "/objs/a.o",
            "/objs/b.o",
            "/sub/deep/",
        ]
    );
}

#[test]
fn nested_repositories_apply_their_own_rules() {
    let tmp = workspace();
    let root = tmp.path().join("outer");
    repo(&root, "vendor/\n*.tmp\n");
    write(root.join("tracked"), "");
    commit_all(&root);

    let nested = root.join("nested");
    repo(&nested, "ntarget/\n");
    write(nested.join("ntarget/z"), "");
    write(nested.join("nfile"), "");
    write(nested.join("x.tmp"), "");
    commit_all(&nested);

    let inner = nested.join("inner");
    repo(&inner, "itarget/\n");
    write(inner.join("itarget/w"), "");
    commit_all(&inner);

    let vendored = root.join("vendor/lib");
    repo(&vendored, "");
    write(vendored.join("v"), "");
    commit_all(&vendored);

    let upstream = tmp.path().join("upstream");
    repo(&upstream, "smbuild/\n");
    write(upstream.join("s"), "");
    commit_all(&upstream);
    git(
        &root,
        &[
            "submodule",
            "add",
            "--quiet",
            upstream.to_str().unwrap(),
            "submod",
        ],
    );
    commit_all(&root);
    write(root.join("submod/smbuild/q"), "");

    assert_eq!(
        excludes(&root),
        [
            "/nested/inner/itarget/",
            "/nested/ntarget/",
            "/submod/smbuild/",
            "/vendor/"
        ]
    );
}

#[test]
fn unversioned_directory_is_treated_as_a_fresh_repository() {
    let tmp = workspace();
    let root = tmp.path().join("plain");
    write(root.join(".gitignore"), "junk/\n*.bak\n");
    write(root.join("proj/.gitignore"), "out/\n");
    for file in ["a.bak", "junk/j", "proj/out/o", "proj/keep"] {
        write(root.join(file), "");
    }
    let nested = root.join("proj/nested");
    repo(&nested, "ntarget/\n");
    write(nested.join("ntarget/z"), "");
    commit_all(&nested);

    assert_eq!(
        excludes(&root),
        ["/a.bak", "/junk/", "/proj/nested/ntarget/", "/proj/out/"]
    );
}

#[test]
fn ignored_sync_root_uses_its_own_rules() {
    let tmp = workspace();
    let root = tmp.path().join("repo");
    repo(&root, "data/\n");
    write(root.join("data/.gitignore"), "*.cache\n");
    write(root.join("data/set.cache"), "");
    write(root.join("data/keep"), "");
    commit_all(&root);

    assert_eq!(excludes(&root.join("data")), ["/set.cache"]);
}

#[test]
fn streamed_rules_reach_rsync_literally() {
    let tmp = workspace();
    let src = tmp.path().join("src");
    repo(
        &src,
        "st\\*ar\nqu\\?\nbr\\[k\\]\nsp ace\nback\\\\slash\nnew?line\nd\\*ir/\n",
    );
    for file in [
        "st*ar",
        "star",
        "qu?",
        "qu1",
        "br[k]",
        "brk",
        "sp ace",
        "space",
        "back\\slash",
        "backslash",
        "new\nline",
        "newline",
        "d*ir/f",
        "dir/f",
    ] {
        write(src.join(file), "");
    }
    commit_all(&src);

    let rules: Vec<Vec<u8>> = git_ignored_paths(&src)
        .unwrap()
        .unwrap()
        .iter()
        .map(|path| [b"- ", path.as_slice()].concat())
        .collect();
    let dst = tmp.path().join("dst");
    sync_directory(
        &format!("{}/", src.display()),
        dst.to_str().unwrap(),
        Rules::Stream(&rules),
        true,
    )
    .unwrap();

    assert_eq!(
        tree(&dst),
        [
            ".gitignore",
            "backslash",
            "brk",
            "dir/f",
            "newline",
            "qu1",
            "space",
            "star"
        ]
    );
}

#[test]
fn argument_rules_let_rsync_read_gitignore_itself() {
    let tmp = workspace();
    let src = tmp.path().join("src");
    write(src.join(".gitignore"), "*.log\n");
    write(src.join("sub/.gitignore"), "out/\n");
    for file in ["a.log", "keep", "sub/out/o", "sub/keep"] {
        write(src.join(file), "");
    }

    let dst = tmp.path().join("dst");
    sync_directory(
        &format!("{}/", src.display()),
        dst.to_str().unwrap(),
        Rules::Args(&[String::from(":- .gitignore")]),
        true,
    )
    .unwrap();

    assert_eq!(
        tree(&dst),
        [".gitignore", "keep", "sub/.gitignore", "sub/keep"]
    );
}
