use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::config::Config;

pub const SYSTEM_PROMPT_FILE: &str = "system_prompt.txt";
pub const COMMAND_DENYLIST_FILE: &str = "command_denylist.txt";
pub const COMMAND_CONFIRMLIST_FILE: &str = "command_confirmlist.txt";
pub const HISTORY_FILE: &str = "history.jsonl";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root_dir: PathBuf,
    pub config_path: PathBuf,
    pub prompt_path: PathBuf,
    pub denylist_path: PathBuf,
    pub confirmlist_path: PathBuf,
    pub history_path: PathBuf,
}

impl AppPaths {
    pub fn from_config_path(config_path: Option<&Path>) -> Result<Self> {
        let config_path = Config::resolve_path(config_path)?;
        let root_dir = config_path
            .parent()
            .map(PathBuf::from)
            .context("The config file must live inside a valid directory")?;

        Ok(Self {
            prompt_path: root_dir.join(SYSTEM_PROMPT_FILE),
            denylist_path: root_dir.join(COMMAND_DENYLIST_FILE),
            confirmlist_path: root_dir.join(COMMAND_CONFIRMLIST_FILE),
            history_path: root_dir.join(HISTORY_FILE),
            root_dir,
            config_path,
        })
    }

    pub fn required_runtime_files(&self) -> Vec<&Path> {
        vec![
            &self.config_path,
            &self.prompt_path,
            &self.denylist_path,
            &self.confirmlist_path,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeResources {
    pub paths: AppPaths,
    pub config: Config,
    pub system_prompt: String,
    pub denylist: Vec<String>,
    pub confirmlist: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InitReport {
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

impl RuntimeResources {
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let paths = AppPaths::from_config_path(config_path)?;

        let missing = paths
            .required_runtime_files()
            .into_iter()
            .filter(|path| !path.exists())
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            bail!(
                "Missing runtime resource files:\n- {}\nRun `atai config init` to initialize the ~/.@ai directory first.",
                missing.join("\n- ")
            );
        }

        let (config, _) = Config::load(Some(paths.config_path.as_path()))?;
        let system_prompt = read_required_text(&paths.prompt_path, "system prompt")?;
        let denylist = read_rule_lines(&paths.denylist_path, "command denylist")?;
        let confirmlist = read_rule_lines(&paths.confirmlist_path, "confirmation list")?;

        Ok(Self {
            paths,
            config,
            system_prompt,
            denylist,
            confirmlist,
        })
    }

    pub fn init(config_path: Option<&Path>) -> Result<InitReport> {
        let paths = AppPaths::from_config_path(config_path)?;
        fs::create_dir_all(&paths.root_dir)
            .with_context(|| format!("Failed to create directory: {}", paths.root_dir.display()))?;

        let mut report = InitReport::default();
        write_if_missing(
            &paths.config_path,
            &Config::commented_template(),
            &mut report,
        )?;
        write_if_missing(&paths.prompt_path, system_prompt_template(), &mut report)?;
        write_if_missing(
            &paths.denylist_path,
            command_denylist_template(),
            &mut report,
        )?;
        write_if_missing(
            &paths.confirmlist_path,
            command_confirmlist_template(),
            &mut report,
        )?;

        Ok(report)
    }
}

fn write_if_missing(path: &Path, content: &str, report: &mut InitReport) -> Result<()> {
    if path.exists() {
        report.skipped.push(path.to_path_buf());
        return Ok(());
    }

    fs::write(path, content)
        .with_context(|| format!("Failed to write initialization file: {}", path.display()))?;
    report.created.push(path.to_path_buf());
    Ok(())
}

fn read_required_text(path: &Path, label: &str) -> Result<String> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {label} file: {}", path.display()))?;
    if text.trim().is_empty() {
        bail!("{label} file is empty: {}", path.display());
    }
    Ok(text)
}

fn read_rule_lines(path: &Path, label: &str) -> Result<Vec<String>> {
    let rules = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {label} file: {}", path.display()))?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_lowercase())
        .collect::<Vec<_>>();

    if rules.is_empty() {
        bail!("{label} file is empty: {}", path.display());
    }

    Ok(rules)
}

fn system_prompt_template() -> &'static str {
    r#"You are a command-line assistant. Convert the user's natural language request into a single POSIX shell command that can be reviewed before execution.

Rules:
1. Return JSON only. No Markdown, no code fences, no extra explanation.
2. The JSON shape must be {"command":"...","summary":"...","assumptions":["..."],"risk_hints":["..."]}.
3. command must be a single-line shell command.
4. Prefer read-only commands. Only return write operations when the user explicitly asks for them.
5. Never use eval, source, backticks, $(), here-docs, background execution, or shell function definitions.
6. Pipes, &&, ||, quotes, and redirections are allowed.
7. summary must be exactly one short English sentence in sentence case, present tense, no markdown, and no trailing period.
8. summary should describe what the command does, not what the user asked for.
9. assumptions must describe tool availability, platform constraints, or filesystem assumptions.
10. risk_hints must list possible risks such as file deletion, overwriting output, or required tools.
11. Commands must match the current platform's default tool behavior. Do not assume GNU-only flags or short-option grouping when an option takes a separate value.
12. Prefer human-readable output when the user asks for sizes, disk usage, or similar metrics. Use flags such as -h unless the user explicitly asks for raw bytes or machine-readable output.
13. Prefer the simplest correct command that a human can read and verify quickly.
"#
}

fn command_denylist_template() -> &'static str {
    r#"# Command denylist
# One keyword per line.
# Any match blocks execution immediately.
mkfs
fdisk
diskutil erase
dd of=/dev/
reboot
shutdown
halt
poweroff
launchctl
"#
}

fn command_confirmlist_template() -> &'static str {
    r#"# High-risk confirmation list
# One keyword per line.
# Any match requires an extra confirmation before execution.
sudo
rm
mv
chmod
chown
find -delete
curl | sh
wget | sh
tee
touch
mkdir
cp
"#
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{AppPaths, RuntimeResources, system_prompt_template};

    #[test]
    fn uses_config_parent_as_runtime_root() {
        let paths =
            AppPaths::from_config_path(Some(Path::new("/tmp/demo/config.toml"))).expect("paths");
        assert_eq!(paths.root_dir, Path::new("/tmp/demo"));
        assert_eq!(paths.prompt_path, Path::new("/tmp/demo/system_prompt.txt"));
    }

    #[test]
    fn init_creates_templates() {
        let temp_dir = std::env::temp_dir().join(format!("atai-init-{}", std::process::id()));
        let config_path = temp_dir.join("config.toml");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        let report = RuntimeResources::init(Some(&config_path)).expect("init");
        assert!(!report.created.is_empty());
        assert!(config_path.exists());
        assert!(temp_dir.join("system_prompt.txt").exists());
        assert!(temp_dir.join("command_denylist.txt").exists());
        assert!(temp_dir.join("command_confirmlist.txt").exists());

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn system_prompt_template_mentions_human_readable_output() {
        let template = system_prompt_template();
        assert!(template.contains("Prefer human-readable output"));
        assert!(template.contains("current platform's default tool behavior"));
    }
}
