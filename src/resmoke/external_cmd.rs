use std::process::{Command, Stdio};

use anyhow::{bail, Result};
use tracing::{event, Level};

/// Run an external command and return the output.
///
/// # Arguments
///
/// * `command` - Command with arguments to run.
///
/// # Return
///
/// The output of the command.
pub fn run_command(command: &[&str]) -> Result<String> {
    let binary = command[0];
    let args = &command[1..];
    let cmd = Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .wait_with_output()?;

    if !cmd.status.success() {
        let error_message = String::from_utf8_lossy(&cmd.stderr).to_string();
        let regular_info = String::from_utf8_lossy(&cmd.stdout).to_string();

        event!(
            Level::ERROR,
            binary = binary,
            args = args.join(" "),
            error_message = error_message,
            stdout = regular_info,
            "Command encountered an error",
        );
        bail!(error_message)
    }

    let output = String::from_utf8_lossy(&cmd.stdout);
    Ok(output.to_string())
}
