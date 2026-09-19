use anyhow::{Context, Result};
use log::{debug, log_enabled, Level, LevelFilter};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};

/// How a transfer's rsync filter rules are handed over.
#[derive(Clone, Copy)]
pub enum Rules<'a> {
    /// As `--filter` arguments; the only form that admits per-directory merge rules such as
    /// `:- .gitignore`, because `--from0` changes how merge files are read.
    Args(&'a [String]),
    /// NUL-separated on stdin, so rules naming any file survive and their number is unbounded.
    Stream(&'a [Vec<u8>]),
}

fn check_rsync_version() -> Result<()> {
    let output = Command::new("rsync")
        .arg("--version")
        .output()
        .context("Failed to execute rsync --version")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get rsync version");
    }

    let version_output = String::from_utf8_lossy(&output.stdout);

    // Parse version from output like "rsync  version 3.2.7  protocol version 31"
    let version_line = version_output
        .lines()
        .next()
        .context("No version information found")?;

    let version_str = version_line
        .split_whitespace()
        .nth(2)
        .context("Could not parse rsync version")?;

    let major_version = version_str
        .split('.')
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .context("Could not parse major version number")?;

    if major_version < 3 {
        anyhow::bail!(
            "rsync version {} is not supported. Please upgrade to version > 3.0",
            version_str
        );
    }
    debug!("rsync version {version_str}");

    Ok(())
}

pub fn get_remote_home(remote_host: &str) -> Result<String> {
    debug!("ssh {remote_host} 'echo $HOME'");
    let output = Command::new("ssh")
        .arg(remote_host)
        .arg("echo $HOME")
        .output()
        .context("Failed to get remote home directory")?;

    if !output.status.success() {
        anyhow::bail!(
            "SSH command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let home = String::from_utf8(output.stdout)?.trim().to_string();

    if home.is_empty() {
        anyhow::bail!("Remote home directory is empty");
    }

    Ok(home)
}

pub fn sync_directory(source: &str, destination: &str, rules: Rules, delete: bool) -> Result<()> {
    let mut cmd = rsync_command(rules, delete);
    cmd.args([source, destination]);
    run_rsync(cmd, rules)
}

pub fn pending_deletions(source: &str, destination: &str, rules: Rules) -> Result<Vec<Vec<u8>>> {
    const DELETING: &[u8] = b"*deleting   ";
    let mut cmd = rsync_command(rules, true);
    cmd.args(["--dry-run", "--out-format=%i %n", source, destination])
        .stdout(Stdio::piped());
    let output = spawn_rsync(cmd, rules)?
        .wait_with_output()
        .context("Failed to wait for rsync")?;
    if !output.status.success() {
        anyhow::bail!("rsync failed with exit code: {:?}", output.status.code());
    }

    Ok(output
        .stdout
        .split(|&b| b == b'\n')
        .filter_map(|line| line.strip_prefix(DELETING))
        .map(|mut name| {
            // rsync prints a byte it finds unprintable as `\#ooo`, the backslash of a literal
            // `\#ooo` included.
            let mut path = Vec::with_capacity(name.len());
            while let Some((&byte, rest)) = name.split_first() {
                match name {
                    [b'\\', b'#', a @ b'0'..=b'3', b @ b'0'..=b'7', c @ b'0'..=b'7', rest @ ..] => {
                        path.push((a - b'0') << 6 | (b - b'0') << 3 | (c - b'0'));
                        name = rest;
                    }
                    _ => {
                        path.push(byte);
                        name = rest;
                    }
                }
            }
            path
        })
        .collect())
}

/// Mirror `path`, relative to `root`, to the same relative location under `destination`.
/// Deletion, when enabled, stays within `path`.
pub fn sync_relative(
    root: &Path,
    path: &str,
    destination: &str,
    rules: Rules,
    delete: bool,
) -> Result<()> {
    let mut cmd = rsync_command(rules, delete);
    cmd.arg("--relative")
        .current_dir(root)
        .args([path, destination]);
    run_rsync(cmd, rules)
}

/// An override path as given on the command line, normalized to `a/b` form. Paths that
/// leave the synced directory are refused.
pub fn override_path(path: &str) -> Result<String> {
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => anyhow::bail!("Override path {path:?} must lie inside the synced directory"),
        }
    }
    if parts.is_empty() {
        anyhow::bail!("Override path {path:?} must name something inside the synced directory");
    }
    Ok(parts.join("/"))
}

fn rsync_command(rules: Rules, delete: bool) -> Command {
    let mut cmd = Command::new("rsync");
    cmd.arg("-az");

    if delete {
        cmd.args(["--delete"]);
    }

    match rules {
        Rules::Args(rules) => {
            for rule in rules {
                cmd.args(["--filter", rule]);
            }
        }
        Rules::Stream(rules) if !rules.is_empty() => {
            cmd.args(["--from0", "--filter", "merge -"]);
            cmd.stdin(Stdio::piped());
        }
        Rules::Stream(_) => {}
    }
    cmd
}

fn run_rsync(mut cmd: Command, rules: Rules) -> Result<()> {
    if log::max_level() < LevelFilter::Info {
        cmd.arg("--quiet");
    } else {
        cmd.arg("-P");
        if log_enabled!(Level::Debug) {
            cmd.arg("--itemize-changes");
        }
    }

    let status = spawn_rsync(cmd, rules)?
        .wait()
        .context("Failed to wait for rsync")?;

    if !status.success() {
        anyhow::bail!("rsync failed with exit code: {:?}", status.code());
    }

    Ok(())
}

fn spawn_rsync(mut cmd: Command, rules: Rules) -> Result<Child> {
    // Ensure rsync version is greater than 3
    check_rsync_version()?;
    debug!("{cmd:?}");

    let mut child = cmd.spawn().context("Failed to execute rsync command")?;

    if let (Some(mut stdin), Rules::Stream(rules)) = (child.stdin.take(), rules) {
        let written = rules
            .iter()
            .try_for_each(|rule| stdin.write_all(rule).and_then(|_| stdin.write_all(b"\0")));
        // rsync that rejected its arguments has already exited; its own status tells why.
        if let Err(err) = written {
            if err.kind() != ErrorKind::BrokenPipe {
                return Err(err).context("Failed to pass filter rules to rsync");
            }
        }
    }

    Ok(child)
}

/// Directories on `host` named `<base>@<anything>`, as full paths.
pub fn list_remote_siblings(host: &str, base: &str) -> Result<Vec<String>> {
    let (parent, name) = match base.rsplit_once('/') {
        Some((parent, name)) if !parent.is_empty() => (parent, name),
        Some((_, name)) => ("/", name),
        None => (".", base),
    };
    let pattern: String = name
        .chars()
        .flat_map(|c| match c {
            '*' | '?' | '[' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .chain("@*".chars())
        .collect();
    let command = format!(
        "find {} -mindepth 1 -maxdepth 1 -type d -name {}",
        shell_quote(parent),
        shell_quote(&pattern)
    );
    debug!("ssh {host} {command}");
    let output = Command::new("ssh")
        .arg(host)
        .arg(&command)
        .output()
        .context("Failed to list remote directories")?;
    if !output.status.success() {
        anyhow::bail!(
            "SSH command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut dirs: Vec<String> = String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect();
    dirs.sort();
    Ok(dirs)
}

pub fn remove_remote_dirs(host: &str, dirs: &[String]) -> Result<()> {
    let quoted: Vec<String> = dirs.iter().map(|dir| shell_quote(dir)).collect();
    execute_ssh_command(host, &format!("rm -rf -- {}", quoted.join(" ")))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn execute_ssh_command(host: &str, command: &str) -> Result<()> {
    debug!("ssh {host} {command}");
    let status = Command::new("ssh")
        .arg(host)
        .arg(command)
        .status()
        .context("Failed to execute SSH command")?;

    if !status.success() {
        anyhow::bail!("SSH command failed with exit code: {:?}", status.code());
    }

    Ok(())
}

pub fn open_remote_shell(host: &str, directory: &str) -> Result<()> {
    let status = Command::new("ssh")
        .arg("-t") // Force pseudo-terminal allocation for interactive shell
        .arg(host)
        .arg(format!("cd {} && exec $SHELL -l", directory))
        .status()
        .context("Failed to open remote shell")?;

    if !status.success() {
        anyhow::bail!("Remote shell exited with code: {:?}", status.code());
    }

    Ok(())
}
