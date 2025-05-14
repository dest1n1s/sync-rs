use anyhow::{Context, Result};
use std::process::Command;

pub fn get_remote_home(remote_host: &str) -> Result<String> {
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

pub fn sync_directory(
    source: &str,
    destination: &str,
    filter: Option<&str>,
    delete: bool,
) -> Result<()> {
    let mut cmd = Command::new("rsync");
    cmd.args(["-azP"]);

    if delete {
        cmd.args(["--delete"]);
    }

    if let Some(f) = filter {
        // Handle multiple filters separated by commas
        for filter_rule in f.split(',') {
            cmd.args(["--filter", filter_rule.trim()]);
        }
    }

    cmd.args([source, destination]);

    let status = cmd.status().context("Failed to execute rsync command")?;

    if !status.success() {
        anyhow::bail!("rsync failed with exit code: {:?}", status.code());
    }

    Ok(())
}

pub fn execute_ssh_command(host: &str, command: &str) -> Result<()> {
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
