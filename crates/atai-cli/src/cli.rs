use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Prompt(String),
    Version,
    Config(ConfigCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    Show,
    Init,
}

#[derive(Debug, Parser)]
#[command(
    name = "atai",
    version,
    about = "Turn natural language requests into reviewable shell commands"
)]
struct RawArgs {
    #[arg(long, help = "Use a custom config path instead of ~/.@ai/config.toml")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Temporarily override the shell path from config")]
    pub shell: Option<PathBuf>,

    #[arg(
        long,
        help = "Print the generated command only; do not open the TUI or execute anything"
    )]
    pub print_only: bool,

    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "The user request or a control command"
    )]
    pub goal: Vec<String>,
}

#[derive(Debug)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub shell: Option<PathBuf>,
    pub print_only: bool,
    pub action: Action,
}

impl Args {
    pub fn parse() -> Result<Self> {
        Self::from_raw(RawArgs::parse())
    }

    fn from_raw(raw: RawArgs) -> Result<Self> {
        let action = parse_action(&raw.goal)?;
        Ok(Self {
            config: raw.config,
            shell: raw.shell,
            print_only: raw.print_only,
            action,
        })
    }

    #[cfg(test)]
    fn try_parse_from<I, T>(iter: I) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let raw =
            RawArgs::try_parse_from(iter).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Self::from_raw(raw)
    }
}

fn parse_action(tokens: &[String]) -> Result<Action> {
    let goal = tokens.join(" ").trim().to_string();
    if goal.is_empty() {
        bail!("The request must not be empty");
    }

    match tokens {
        [single] if single == "version" => Ok(Action::Version),
        [single] if single == "config" => Ok(Action::Config(ConfigCommand::Show)),
        [prefix, suffix] if prefix == "config" && suffix == "show" => {
            Ok(Action::Config(ConfigCommand::Show))
        }
        [prefix, suffix] if prefix == "config" && suffix == "init" => {
            Ok(Action::Config(ConfigCommand::Init))
        }
        _ => Ok(Action::Prompt(goal)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Args, ConfigCommand};

    #[test]
    fn parses_version_command() {
        let args = Args::try_parse_from(["atai", "version"]).expect("parse version");
        assert_eq!(args.action, Action::Version);
    }

    #[test]
    fn parses_config_show_as_default() {
        let args = Args::try_parse_from(["atai", "config"]).expect("parse config show");
        assert_eq!(args.action, Action::Config(ConfigCommand::Show));
    }

    #[test]
    fn parses_config_init_command() {
        let args = Args::try_parse_from(["atai", "config", "init"]).expect("parse config init");
        assert_eq!(args.action, Action::Config(ConfigCommand::Init));
    }

    #[test]
    fn keeps_natural_language_as_prompt() {
        let args = Args::try_parse_from(["atai", "find", "the", "largest", "directories"])
            .expect("parse prompt");
        assert_eq!(
            args.action,
            Action::Prompt("find the largest directories".to_string())
        );
    }
}
