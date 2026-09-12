use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, process};

/// One entry of `git worktree list`.
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// A repository's worktrees, the main one first.
pub struct Repo {
    worktrees: Vec<Worktree>,
}

impl Repo {
    pub fn main(&self) -> &Worktree {
        &self.worktrees[0]
    }

    pub fn on_branch(&self, branch: &str) -> Option<&Worktree> {
        self.worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some(branch))
    }

    fn containing(&self, dir: &Path) -> Option<usize> {
        (0..self.worktrees.len())
            .filter(|&i| dir.starts_with(&self.worktrees[i].path))
            .max_by_key(|&i| self.worktrees[i].path.components().count())
    }
}

struct Placement {
    repo: Repo,
    /// Index of the worktree holding `cwd`.
    current: usize,
    /// `cwd` relative to that worktree.
    rel: PathBuf,
}

/// The directory sync-rs was started in, placed among its repository's worktrees.
pub struct Location {
    cwd: PathBuf,
    context: Option<Placement>,
}

impl Location {
    pub fn plain(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            context: None,
        }
    }

    /// Plain when git is not installed or `cwd` is not inside a work tree.
    pub fn discover(cwd: &Path) -> Result<Self> {
        let canonical = cwd.canonicalize()?;
        let output = match Command::new("git")
            .arg("-C")
            .arg(&canonical)
            .args(["worktree", "list", "--porcelain"])
            .output()
        {
            Ok(output) => output,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Self::plain(cwd)),
            Err(err) => return Err(err).context("Failed to execute git"),
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not a git repository") {
                return Ok(Self::plain(cwd));
            }
            bail!("git worktree list failed: {}", stderr.trim());
        }
        let repo = Repo {
            worktrees: parse_worktrees(&String::from_utf8_lossy(&output.stdout)),
        };
        let Some(current) = repo.containing(&canonical) else {
            return Ok(Self::plain(cwd));
        };
        let rel = canonical
            .strip_prefix(&repo.worktrees[current].path)
            .unwrap()
            .to_path_buf();
        Ok(Self {
            cwd: cwd.to_path_buf(),
            context: Some(Placement { repo, current, rel }),
        })
    }

    /// The directory whose remote configuration applies: `cwd` itself, or its counterpart
    /// under the main worktree when `cwd` lies in a linked one.
    pub fn config_dir(&self) -> PathBuf {
        match &self.context {
            Some(context) if context.current != 0 => under(&context.repo.main().path, &context.rel),
            _ => self.cwd.clone(),
        }
    }

    /// What to sync. Without `branch` it is `cwd`; with one it is the same subdirectory of
    /// the worktree checked out on `branch`, created as a temporary detached checkout when
    /// no worktree has it. Anything but the main worktree carries a suffix naming its
    /// branch (or its directory when detached) for the remote directory.
    pub fn target(&self, branch: Option<&str>) -> Result<Target> {
        let Some(context) = &self.context else {
            if branch.is_some() {
                bail!("--branch requires a git repository");
            }
            return Ok(Target {
                source: self.cwd.clone(),
                suffix: None,
                temp: None,
            });
        };
        let Some(branch) = branch else {
            let current = &context.repo.worktrees[context.current];
            let suffix = (context.current != 0).then(|| match &current.branch {
                Some(branch) => suffix_for(branch),
                None => current
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            });
            return Ok(Target {
                source: self.cwd.clone(),
                suffix,
                temp: None,
            });
        };
        if let Some(worktree) = context.repo.on_branch(branch) {
            let source = under(&worktree.path, &context.rel);
            if !source.is_dir() {
                bail!(
                    "{} does not exist in the worktree on {branch} ({})",
                    context.rel.display(),
                    worktree.path.display()
                );
            }
            let is_main = std::ptr::eq(worktree, context.repo.main());
            return Ok(Target {
                source,
                suffix: (!is_main).then(|| suffix_for(branch)),
                temp: None,
            });
        }
        let temp = TempWorktree::add(&context.repo.main().path, branch)?;
        let source = under(&temp.path, &context.rel);
        if !source.is_dir() {
            bail!("{} does not exist on {branch}", context.rel.display());
        }
        Ok(Target {
            source,
            suffix: Some(suffix_for(branch)),
            temp: Some(temp),
        })
    }
}

impl Location {
    /// Every suffix a sync from this repository could currently produce: its linked worktrees
    /// plus all branches, tags and remote-tracking refs. `None` outside a repository.
    pub fn live_suffixes(&self) -> Result<Option<HashSet<String>>> {
        let Some(context) = &self.context else {
            return Ok(None);
        };
        let mut suffixes: HashSet<String> = context.repo.worktrees[1..]
            .iter()
            .map(|worktree| match &worktree.branch {
                Some(branch) => suffix_for(branch),
                None => worktree
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect();
        let output = Command::new("git")
            .arg("-C")
            .arg(&context.repo.main().path)
            .args(["for-each-ref", "--format=%(refname:short)"])
            .output()
            .context("Failed to execute git for-each-ref")?;
        if !output.status.success() {
            bail!(
                "git for-each-ref failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        suffixes.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(suffix_for),
        );
        Ok(Some(suffixes))
    }
}

/// Where a sync reads from; a temporary checkout is removed when this is dropped.
pub struct Target {
    pub source: PathBuf,
    pub suffix: Option<String>,
    #[allow(dead_code)]
    temp: Option<TempWorktree>,
}

/// `base/rel`, or `base` itself when `rel` is empty (`Path::join` would add a trailing slash).
fn under(base: &Path, rel: &Path) -> PathBuf {
    if rel.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(rel)
    }
}

/// The remote-directory suffix naming `branch`.
pub fn suffix_for(branch: &str) -> String {
    branch.replace('/', "-")
}

fn parse_worktrees(listing: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    for block in listing
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
    {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let Some(path) = path {
            worktrees.push(Worktree { path, branch });
        }
    }
    worktrees
}

/// A detached checkout under the temp dir, registered with the repository until dropped.
struct TempWorktree {
    repo: PathBuf,
    path: PathBuf,
}

impl TempWorktree {
    fn add(repo: &Path, branch: &str) -> Result<Self> {
        let path = env::temp_dir().canonicalize()?.join(format!(
            "sync-rs-{}-{}",
            process::id(),
            suffix_for(branch)
        ));
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "add", "--quiet", "--detach"])
            .arg(&path)
            .arg(branch)
            .output()
            .context("Failed to execute git worktree add")?;
        if !output.status.success() {
            bail!(
                "Could not check out {branch}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Self {
            repo: repo.to_path_buf(),
            path,
        })
    }
}

impl Drop for TempWorktree {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
    }
}
