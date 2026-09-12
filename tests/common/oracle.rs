//! An independent reading of what git keeps under a directory, built on `git check-ignore`
//! rather than the `ls-files` walk the implementation relies on.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::git;

/// Files under `sync_root` that git would keep, relative to it and sorted: a file survives when
/// its owning repository does not ignore it and no enclosing nested-repository directory is
/// ignored by *its* owner. A `sync_root` outside any work tree, or ignored by its owner, is
/// `git init`ed first so it is judged as a fresh repository. Repositories are only searched up
/// to `stop`. A directory that is both a repository and tracked as plain files by an enclosing
/// repository has no single answer and must not be presented.
pub fn expected_files(sync_root: &Path, stop: &Path) -> Vec<String> {
    let mut top = sync_root
        .ancestors()
        .take_while(|dir| dir.starts_with(stop))
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf);
    let fresh = match &top {
        None => true,
        Some(top) => {
            top != sync_root && !ignored_in(top, &[sync_root.strip_prefix(top).unwrap()]).is_empty()
        }
    };
    if fresh {
        git(sync_root, &["init", "--quiet"]);
        top = Some(sync_root.to_path_buf());
    }
    let top = top.unwrap();

    let mut files = Vec::new();
    let mut repos = vec![top.clone()];
    walk(sync_root, &mut files, &mut repos);
    repos.sort();
    repos.dedup();
    let owner_of = |path: &Path| -> PathBuf {
        repos
            .iter()
            .filter(|repo| path.starts_with(repo))
            .max_by_key(|repo| repo.components().count())
            .unwrap()
            .clone()
    };

    let mut queries: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for repo in repos.iter().filter(|repo| **repo != top) {
        let owner = owner_of(repo.parent().unwrap());
        queries
            .entry(owner.clone())
            .or_default()
            .push(repo.strip_prefix(&owner).unwrap().to_path_buf());
    }
    for file in &files {
        let owner = owner_of(file);
        queries
            .entry(owner.clone())
            .or_default()
            .push(file.strip_prefix(&owner).unwrap().to_path_buf());
    }
    let ignored: HashMap<PathBuf, HashSet<PathBuf>> = queries
        .into_iter()
        .map(|(repo, paths)| {
            let set = ignored_in(
                &repo,
                &paths.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
            );
            (repo, set)
        })
        .collect();
    let is_ignored = |owner: &Path, path: &Path| {
        ignored
            .get(owner)
            .is_some_and(|set| set.contains(path.strip_prefix(owner).unwrap()))
    };

    let mut excluded_repo: HashMap<PathBuf, bool> = HashMap::new();
    for repo in &repos {
        let excluded = if *repo == top {
            false
        } else {
            let owner = owner_of(repo.parent().unwrap());
            is_ignored(&owner, repo) || excluded_repo[&owner]
        };
        excluded_repo.insert(repo.clone(), excluded);
    }

    let mut kept: Vec<String> = files
        .iter()
        .filter(|file| {
            let owner = owner_of(file);
            !is_ignored(&owner, file) && !excluded_repo[&owner]
        })
        .map(|file| {
            file.strip_prefix(sync_root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    kept.sort();
    kept
}

/// Files (symlinks included) and repository directories below `dir`, skipping git metadata.
fn walk(dir: &Path, files: &mut Vec<PathBuf>, repos: &mut Vec<PathBuf>) {
    if dir.join(".git").exists() {
        repos.push(dir.to_path_buf());
    }
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == ".git" {
            continue;
        }
        if fs::symlink_metadata(&path).unwrap().is_dir() {
            walk(&path, files, repos);
        } else {
            files.push(path);
        }
    }
}

/// The subset of `paths` (relative to `repo`) that `git check-ignore` reports as ignored.
fn ignored_in(repo: &Path, paths: &[&Path]) -> HashSet<PathBuf> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for path in paths {
            stdin.write_all(path.as_os_str().as_bytes()).unwrap();
            stdin.write_all(b"\0").unwrap();
        }
    }
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    let status = child.wait().unwrap();
    assert!(
        matches!(status.code(), Some(0 | 1)),
        "git check-ignore failed in {}",
        repo.display()
    );
    out.split(|&b| b == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| PathBuf::from(OsStr::from_bytes(entry)))
        .collect()
}
