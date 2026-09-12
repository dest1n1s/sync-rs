use anyhow::{bail, Context, Result};
use log::{debug, trace};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, process};

const IGNORED: &[&str] = &["--others", "--ignored", "--exclude-standard", "--directory"];

/// Paths under `root` that git ignores, as rsync exclude patterns anchored at `root`
/// (`/target/`, `/notes/draft.log`). Nested repositories, submodules or plain checkouts alike,
/// contribute their own rules for the subtree they own. A `root` outside any work tree, or one
/// that an enclosing repository ignores wholesale, is evaluated as if freshly `git init`ed, so
/// only the `.gitignore` files beneath it apply. `None` when git is not installed.
pub fn git_ignored_paths(root: &Path) -> Result<Option<Vec<Vec<u8>>>> {
    let root = root.canonicalize()?;
    let probe = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("Failed to execute git"),
    };
    let stderr = String::from_utf8_lossy(&probe.stderr);
    if !probe.status.success() && !stderr.contains("not a git repository") {
        bail!("git rev-parse failed: {}", stderr.trim());
    }

    let mut repo = Repo {
        dir: root,
        scratch: None,
    };
    let mut ignored = if probe.stdout.trim_ascii() == b"true" {
        repo.ls_files(IGNORED)?
    } else {
        Vec::new()
    };
    // Git reports an ignored root as a lone `./` and never looks inside it.
    if probe.stdout.trim_ascii() != b"true" || ignored.iter().any(|entry| entry == b"./") {
        debug!(
            "{} is not a work tree of its own; evaluating .gitignore files against a scratch index",
            repo.dir.display()
        );
        repo.scratch = Some(ScratchRepo::create()?);
        ignored = repo.ls_files(IGNORED)?;
    }

    let mut excludes = Vec::new();
    collect(&repo, ignored, b"", &mut excludes)?;
    Ok(Some(excludes))
}

fn collect(
    repo: &Repo,
    ignored: Vec<Vec<u8>>,
    prefix: &[u8],
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    // An ignored directory is listed together with its contents; the directory alone suffices.
    let mut excluded_dir: Option<Vec<u8>> = None;
    for entry in ignored {
        if excluded_dir
            .as_ref()
            .is_some_and(|dir| entry.starts_with(dir))
        {
            continue;
        }
        if entry.ends_with(b"/") {
            excluded_dir = Some(entry.clone());
        }
        let pattern = exclude_pattern(prefix, &entry);
        trace!("exclude {}", String::from_utf8_lossy(&pattern));
        out.push(pattern);
    }
    for nested in repo.nested_repos()? {
        debug!(
            "nested repository {}{}",
            String::from_utf8_lossy(prefix),
            String::from_utf8_lossy(&nested)
        );
        let sub = Repo {
            dir: repo.dir.join(OsStr::from_bytes(&nested)),
            scratch: None,
        };
        let ignored = sub.ls_files(IGNORED)?;
        collect(&sub, ignored, &[prefix, &nested, b"/"].concat(), out)?;
    }
    Ok(())
}

/// `/prefix/entry` with rsync's wildcard characters escaped so the path matches literally.
/// A backslash is itself an escape only in patterns that contain a wildcard.
fn exclude_pattern(prefix: &[u8], entry: &[u8]) -> Vec<u8> {
    let path = [prefix, entry].concat();
    let mut pattern = vec![b'/'];
    if path.iter().any(|b| matches!(b, b'*' | b'?' | b'[')) {
        for &b in &path {
            if matches!(b, b'*' | b'?' | b'[' | b'\\') {
                pattern.push(b'\\');
            }
            pattern.push(b);
        }
    } else {
        pattern.extend_from_slice(&path);
    }
    pattern
}

struct Repo {
    dir: PathBuf,
    scratch: Option<ScratchRepo>,
}

impl Repo {
    fn git(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.dir);
        if let Some(scratch) = &self.scratch {
            cmd.arg("--git-dir")
                .arg(&scratch.path)
                .arg("--work-tree")
                .arg(&self.dir);
        }
        cmd
    }

    /// `git ls-files -z` entries, relative to `dir`.
    fn ls_files(&self, args: &[&str]) -> Result<Vec<Vec<u8>>> {
        let output = self
            .git()
            .args(["ls-files", "-z"])
            .args(args)
            .output()
            .context("Failed to execute git ls-files")?;
        if !output.status.success() {
            bail!(
                "git ls-files failed in {}: {}",
                self.dir.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let entries: Vec<Vec<u8>> = output
            .stdout
            .split(|&b| b == 0)
            .filter(|entry| !entry.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        debug!(
            "git ls-files {} in {}: {} entries",
            args.join(" "),
            self.dir.display(),
            entries.len()
        );
        Ok(entries)
    }

    /// Directories holding a repository of their own: submodules (gitlinks in the index) and
    /// untracked checkouts, which git lists as a bare `dir/` instead of descending into them.
    fn nested_repos(&self) -> Result<Vec<Vec<u8>>> {
        let mut dirs = Vec::new();
        for entry in self.ls_files(&["--others", "--exclude-standard"])? {
            if let Some(dir) = entry.strip_suffix(b"/") {
                dirs.push(dir.to_vec());
            }
        }
        for entry in self.ls_files(&["--stage"])? {
            let path = entry.strip_prefix(b"160000 ").and_then(|rest| {
                rest.iter()
                    .position(|&b| b == b'\t')
                    .map(|i| &rest[i + 1..])
            });
            if let Some(path) = path {
                dirs.push(path.to_vec());
            }
        }
        dirs.retain(|dir| self.dir.join(OsStr::from_bytes(dir)).join(".git").exists());
        Ok(dirs)
    }
}

/// A bare repository under the temp dir; its empty index makes git treat everything in the
/// work tree it is pointed at as untracked. Removed on drop.
struct ScratchRepo {
    path: PathBuf,
}

impl ScratchRepo {
    fn create() -> Result<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!("sync-rs-{}-{nanos}.git", process::id()));
        let output = Command::new("git")
            .args(["init", "--quiet", "--bare"])
            .arg(&path)
            .output()
            .context("Failed to execute git init")?;
        if !output.status.success() {
            bail!(
                "git init failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Self { path })
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
