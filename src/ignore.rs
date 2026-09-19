use anyhow::{bail, Context, Result};
use log::{debug, trace};
use std::ffi::OsStr;
use std::io::{ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, process, thread};

const IGNORED: &[&str] = &["--others", "--ignored", "--exclude-standard", "--directory"];

/// Paths under `root` that git ignores, as rsync exclude patterns anchored at `root`
/// (`/target/`, `/notes/draft.log`). Nested repositories, submodules or plain checkouts alike,
/// contribute their own rules for the subtree they own. A `root` outside any work tree, or one
/// that an enclosing repository ignores wholesale, is evaluated as if freshly `git init`ed, so
/// only the `.gitignore` files beneath it apply. `None` when git is not installed.
pub fn git_ignored_paths(root: &Path) -> Result<Option<Vec<Vec<u8>>>> {
    let Some((repo, ignored)) = Repo::open(root)? else {
        return Ok(None);
    };
    let mut excludes = Vec::new();
    collect(&repo, ignored, b"", &mut excludes)?;
    Ok(Some(excludes))
}

pub fn git_ignored_among(root: &Path, candidates: &[Vec<u8>]) -> Result<Vec<Vec<u8>>> {
    let mut excludes = Vec::new();
    if candidates.is_empty() {
        return Ok(excludes);
    }
    let (repo, _) = Repo::open(root)?.context("git is not installed")?;
    // Whatever a local file or symlink replaces goes with it.
    let candidates = candidates
        .iter()
        .map(Vec::as_slice)
        .filter(|path| {
            let path = path.strip_suffix(b"/").unwrap_or(path);
            !Path::new(OsStr::from_bytes(path)).ancestors().any(|at| {
                repo.dir
                    .join(at)
                    .symlink_metadata()
                    .is_ok_and(|meta| !meta.is_dir())
            })
        })
        .collect();
    collect_among(&repo, candidates, b"", &mut excludes)?;
    Ok(excludes)
}

fn collect(
    repo: &Repo,
    ignored: Vec<Vec<u8>>,
    prefix: &[u8],
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    push_excludes(ignored, prefix, out);
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

fn collect_among(
    repo: &Repo,
    mut candidates: Vec<&[u8]>,
    prefix: &[u8],
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    for nested in repo.nested_repos()? {
        let dir = [&nested[..], b"/"].concat();
        let (inside, outside): (Vec<&[u8]>, Vec<&[u8]>) = candidates
            .into_iter()
            .partition(|path| path.starts_with(&dir));
        candidates = outside;
        if inside.is_empty() {
            continue;
        }
        let sub = Repo {
            dir: repo.dir.join(OsStr::from_bytes(&nested)),
            scratch: None,
        };
        let inside = inside.into_iter().map(|path| &path[dir.len()..]).collect();
        collect_among(&sub, inside, &[prefix, &dir].concat(), out)?;
    }
    let mut ignored = repo.check_ignore(&candidates)?;
    ignored.sort();
    push_excludes(ignored, prefix, out);
    Ok(())
}

fn push_excludes(ignored: Vec<Vec<u8>>, prefix: &[u8], out: &mut Vec<Vec<u8>>) {
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
}

/// `/prefix/entry` with rsync's wildcard characters escaped so the path matches literally.
/// A backslash is itself an escape only in patterns that contain a wildcard.
pub fn exclude_pattern(prefix: &[u8], entry: &[u8]) -> Vec<u8> {
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
    fn open(root: &Path) -> Result<Option<(Self, Vec<Vec<u8>>)>> {
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
        Ok(Some((repo, ignored)))
    }

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

    fn check_ignore(&self, paths: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut child = self
            .git()
            .args(["check-ignore", "-z", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute git check-ignore")?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let input = paths.join(&0);
        let writer = thread::spawn(move || stdin.write_all(&input));
        let output = child
            .wait_with_output()
            .context("Failed to wait for git check-ignore")?;
        let _ = writer.join();
        // Exit status 1 only says that nothing is ignored.
        if !matches!(output.status.code(), Some(0 | 1)) {
            bail!(
                "git check-ignore failed in {}: {}",
                self.dir.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output
            .stdout
            .split(|&b| b == 0)
            .filter(|entry| !entry.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
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
