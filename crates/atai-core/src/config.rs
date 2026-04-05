use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

static ENV_EXPR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$").expect("env regex"));

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: ModelConfig,
    pub execution: ExecutionConfig,
    pub safety: SafetyConfig,
    pub history: HistoryConfig,
}

impl Config {
    pub fn read(path: Option<&Path>) -> Result<(Self, PathBuf)> {
        let path = Self::resolve_path(path)?;
        let raw = fs::read_to_string(&path).with_context(|| {
            format!(
                "Config file not found: {}\nRun `atai config init` to initialize the ~/.@ai directory first.",
                path.display(),
            )
        })?;

        let config: Self = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok((config, path))
    }

    pub fn load(path: Option<&Path>) -> Result<(Self, PathBuf)> {
        let (config, path) = Self::read(path)?;
        config.validate()?;
        Ok((config, path))
    }

    pub fn default_config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn resolve_path(path: Option<&Path>) -> Result<PathBuf> {
        Ok(path
            .map(PathBuf::from)
            .unwrap_or_else(|| Self::default_config_path().expect("default config path")))
    }

    pub fn config_dir() -> Result<PathBuf> {
        dirs::home_dir()
            .map(|path| path.join(".@ai"))
            .context("Failed to locate the user's home directory")
    }

    fn validate(&self) -> Result<()> {
        if self.model.endpoint.trim().is_empty() {
            bail!("model.endpoint must not be empty");
        }
        if self.model.model.trim().is_empty() {
            bail!("model.model must not be empty");
        }
        if self.model.timeout_seconds == 0 {
            bail!("model.timeout_seconds must be greater than 0");
        }
        ApiKey::resolve(&self.model.api_key)?;
        Ok(())
    }

    pub fn example_config() -> String {
        r#"[model]
endpoint = "https://api.openai.com/v1"
model = "gpt-5.4"
api_key = "${OPENAI_API_KEY}"
timeout_seconds = 30
# enable_thinking = false

[execution]
shell = "/bin/zsh"

[safety]
mode = "tiered"

[history]
enabled = false
max_entries = 200
redact_paths = true
"#
        .to_string()
    }

    pub fn commented_template() -> String {
        r#"# @ai configuration file
# Default path: ~/.@ai/config.toml
#
# Notes:
# 1. endpoint must point to an OpenAI-compatible /v1 API.
# 2. api_key supports two formats:
#    - "${OPENAI_API_KEY}" reads from an environment variable
#    - "sk-..." uses the literal string directly
# 3. The remaining runtime files live in the same directory:
#    - system_prompt.txt
#    - command_denylist.txt
#    - command_confirmlist.txt
# 4. history is disabled by default to avoid writing local paths and commands to disk.

[model]
# Base URL for OpenAI or an OpenAI-compatible gateway, usually ending in /v1.
endpoint = "https://api.openai.com/v1"

# Model name to call.
model = "gpt-5.4"

# Using an environment variable is recommended to avoid storing the key on disk.
api_key = "${OPENAI_API_KEY}"

# Timeout for a single request, in seconds.
timeout_seconds = 60

# Optional: explicitly control provider/model thinking mode.
# - false: disable thinking on supported providers to reduce latency
# - true: enable provider-native thinking mode when supported
# - omitted: keep the provider default behavior
# enable_thinking = false

[execution]
# Shell used to execute commands.
shell = "/bin/zsh"

[safety]
# Supported values:
# - "tiered": denylist blocks immediately, high-risk commands require an extra confirmation
# - "strict": any high-risk match is denied directly
mode = "tiered"

[history]
# Whether to write ~/.@ai/history.jsonl
enabled = false

# Maximum number of history entries to retain.
max_entries = 200

# Replace the home directory and current working directory with placeholders.
redact_paths = true
"#
        .to_string()
    }

    pub fn render_for_display(&self, path: &Path) -> String {
        let mut rendered = format!(
            "# Config file: {}\n# Note: api_key is masked for display; runtime still resolves ${{VAR}} expressions.\n\n[model]\nendpoint = \"{}\"\nmodel = \"{}\"\napi_key = \"{}\"\ntimeout_seconds = {}\n",
            path.display(),
            escape_toml_string(&self.model.endpoint),
            escape_toml_string(&self.model.model),
            escape_toml_string(&display_api_key(&self.model.api_key)),
            self.model.timeout_seconds,
        );

        if let Some(enable_thinking) = self.model.enable_thinking {
            rendered.push_str(&format!("enable_thinking = {enable_thinking}\n"));
        } else {
            rendered.push_str("# enable_thinking = <provider default>\n");
        }

        rendered.push_str(&format!(
            "\n[execution]\nshell = \"{}\"\n\n[safety]\nmode = \"{}\"\n\n[history]\nenabled = {}\nmax_entries = {}\nredact_paths = {}\n",
            escape_toml_string(&self.execution.shell),
            escape_toml_string(&self.safety.mode),
            self.history.enabled,
            self.history.max_entries,
            self.history.redact_paths,
        ));

        rendered
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub timeout_seconds: u64,
    pub enable_thinking: Option<bool>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1".to_string(),
            model: "gpt-5.4".to_string(),
            api_key: "${OPENAI_API_KEY}".to_string(),
            timeout_seconds: 60,
            enable_thinking: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ExecutionConfig {
    pub shell: String,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            shell: env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        }
    }
}

impl ExecutionConfig {
    pub fn shell_path(&self) -> PathBuf {
        PathBuf::from(self.shell.trim())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    pub mode: String,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            mode: "tiered".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub redact_paths: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 200,
            redact_paths: true,
        }
    }
}

pub struct ApiKey;

impl ApiKey {
    pub fn resolve(raw: &str) -> Result<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("model.api_key must not be empty");
        }

        if let Some(captures) = ENV_EXPR.captures(trimmed) {
            let name = captures
                .get(1)
                .map(|matched| matched.as_str())
                .context("Failed to parse the environment variable expression")?;
            let value = env::var(name).with_context(|| {
                format!("The configuration expects API key from environment variable {name}, but that variable is not set")
            })?;
            if value.trim().is_empty() {
                bail!("Environment variable {name} is empty");
            }
            return Ok(value);
        }

        Ok(trimmed.to_string())
    }
}

fn display_api_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if ENV_EXPR.is_match(trimmed) {
        return trimmed.to_string();
    }

    let suffix = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    if suffix.is_empty() {
        "***".to_string()
    } else {
        format!("***{suffix}")
    }
}

fn escape_toml_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ApiKey, Config};

    #[test]
    fn resolves_plain_api_key() {
        let key = ApiKey::resolve("sk-test").expect("plain key");
        assert_eq!(key, "sk-test");
    }

    #[test]
    fn resolves_env_api_key() {
        let expected = std::env::var("PATH").expect("PATH should exist in test env");
        let key = ApiKey::resolve("${PATH}").expect("env key");
        assert_eq!(key, expected);
    }

    #[test]
    fn commented_template_contains_explanatory_comments() {
        let template = Config::commented_template();
        assert!(template.contains("# @ai configuration file"));
        assert!(template.contains("api_key = \"${OPENAI_API_KEY}\""));
        assert!(template.contains("# enable_thinking = false"));
    }

    #[test]
    fn render_for_display_masks_plain_api_key() {
        let mut config = Config::default();
        config.model.api_key = "sk-secret-value".to_string();

        let rendered = config.render_for_display(Path::new("/tmp/config.toml"));

        assert!(rendered.contains("api_key = \"***alue\""));
        assert!(!rendered.contains("sk-secret-value"));
        assert!(rendered.contains("# enable_thinking = <provider default>"));
    }

    #[test]
    fn render_for_display_includes_enable_thinking_when_configured() {
        let mut config = Config::default();
        config.model.enable_thinking = Some(false);

        let rendered = config.render_for_display(Path::new("/tmp/config.toml"));

        assert!(rendered.contains("enable_thinking = false"));
    }
}
