use anyhow::{bail, Result};
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{event, Level};
use wait_timeout::ChildExt;

/// Run an external command and return the output.
///
/// # Arguments
///
/// * `command` - Command with arguments to run.
/// * `timeout` - A duration before the command times out. If None, defaults to 120s.
///
/// # Return
///
/// The output of the command.
pub fn run_command(command: &[&str], timeout: Option<Duration>) -> Result<String> {
    let timeout = match timeout {
        Some(timeout) => timeout,
        None => Duration::from_secs(120),
    };

    let binary = command[0];
    let args = &command[1..];
    let mut cmd = Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let timed_out = match cmd.wait_timeout(timeout).unwrap() {
        Some(_) => false,
        None => true,
    };

    let output = cmd.wait_with_output()?;

    if !output.status.success() || timed_out {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let message = if timed_out {
            format!("Command timed out after {} seconds.", timeout.as_secs()).to_string()
        } else {
            stderr.clone()
        };

        event!(
            Level::ERROR,
            binary = binary,
            args = args.join(" "),
            stderr = stderr,
            stdout = stdout,
            "{}",
            message
        );
        bail!(message)
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::run_command;
    use std::time::Duration;

    #[test]
    fn run_command_basic() {
        let cmd = vec!["echo", "hello"];
        assert_eq!(run_command(&cmd, None).unwrap(), "hello\n");
    }

    #[test]
    fn run_command_error() {
        let cmd = vec!["i_do_not_exist"];
        let result = run_command(&cmd, None);
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("No such file or directory"));
    }

    #[test]
    fn run_command_timeout() {
        let cmd = vec!["sleep", "1"];
        let result = run_command(&cmd, Some(Duration::from_millis(100)));
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Command timed out after 0 seconds."
        );
    }
}
