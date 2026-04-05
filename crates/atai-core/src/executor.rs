use std::{path::Path, process::Command, time::Instant};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u128,
}

pub struct Executor;

impl Executor {
    pub fn run(shell: &Path, command: &str, cwd: &Path) -> Result<RunResult> {
        let started_at = Instant::now();
        let output = Command::new(shell)
            .arg("-lc")
            .arg(command)
            .current_dir(cwd)
            .output()
            .with_context(|| {
                format!("Failed to execute command with shell: {}", shell.display())
            })?;

        Ok(RunResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            duration_ms: started_at.elapsed().as_millis(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Executor;

    #[test]
    fn runs_command_and_captures_stdout() {
        let result =
            Executor::run(Path::new("/bin/sh"), "printf 'hello'", Path::new(".")).expect("run");

        assert_eq!(result.stdout, "hello");
        assert_eq!(result.exit_code, 0);
    }
}
