use std::{
    fmt,
    path::{Path, PathBuf},
};

use once_cell::sync::Lazy;
use regex::Regex;

static EVAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(^|[|;&]\s*)eval\b").expect("eval regex"));
static SOURCE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(^|[|;&]\s*)source\b").expect("source regex"));
static FUNCTION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\bfunction\b|\(\)\s*\{").expect("function regex"));
static RM_ROOT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\brm\b[^\n]*\s(/|~|\$HOME)(\s|$)").expect("rm root regex"));
static REDIRECTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?x)
        (?:^|[\s|&])
        (?:\d*>>? | &>>? )
        \s*
        (?P<target>"[^"]+"|'[^']+'|[^\s|&;]+)
    "#,
    )
    .expect("redirection regex")
});

#[derive(Clone, Debug)]
pub struct PolicyRules {
    pub denylist: Vec<String>,
    pub confirmlist: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,
    HighRisk,
    Deny,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => write!(f, "SAFE"),
            Self::HighRisk => write!(f, "HIGH_RISK"),
            Self::Deny => write!(f, "DENY"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RiskReport {
    pub level: RiskLevel,
    pub reasons: Vec<String>,
}

pub struct PolicyEngine {
    mode: String,
    denylist: Vec<String>,
    confirmlist: Vec<String>,
    home_dir: Option<PathBuf>,
}

impl PolicyEngine {
    pub fn new(mode: String, rules: PolicyRules) -> Self {
        Self {
            mode,
            denylist: rules.denylist,
            confirmlist: rules.confirmlist,
            home_dir: dirs::home_dir(),
        }
    }

    pub fn classify(&self, command: &str, cwd: &Path) -> RiskReport {
        let trimmed = command.trim();
        let lowered = trimmed.to_lowercase();
        let mut deny = Vec::new();
        let mut high_risk = Vec::new();

        if trimmed.is_empty() {
            deny.push("The command is empty".to_string());
        }

        if trimmed.contains('\n') {
            deny.push("Multi-line commands are not allowed".to_string());
        }

        if trimmed.contains(';') {
            deny.push("Using ';' to chain commands is not allowed".to_string());
        }

        if trimmed.contains('`') || trimmed.contains("$(") {
            deny.push("Dynamic execution via backticks or $() is not allowed".to_string());
        }

        if trimmed.contains("<<") {
            deny.push("here-doc and here-string syntax is not allowed".to_string());
        }

        if has_background_operator(trimmed) {
            deny.push("Background execution is not allowed".to_string());
        }

        if EVAL_RE.is_match(&lowered) || SOURCE_RE.is_match(&lowered) {
            deny.push("Using eval or source is not allowed".to_string());
        }

        if FUNCTION_RE.is_match(trimmed) {
            deny.push("Shell function definitions are not allowed".to_string());
        }

        if RM_ROOT_RE.is_match(trimmed) || self.targets_home_root(trimmed) {
            deny.push("Matched the catastrophic deletion deny rule".to_string());
        }

        for rule in &self.denylist {
            if lowered.contains(rule) {
                deny.push(format!("Matched a custom denylist rule: {rule}"));
            }
        }

        if self.redirects_to_regular_file(trimmed) {
            high_risk
                .push("The command uses redirection and may overwrite or write files".to_string());
        }

        if self.touches_path_outside_cwd(trimmed, cwd) {
            high_risk
                .push("The command may write outside the current working directory".to_string());
        }

        for rule in &self.confirmlist {
            if lowered.contains(rule) {
                high_risk.push(format!(
                    "Matched a custom high-risk confirmation rule: {rule}"
                ));
            }
        }

        let level = if !deny.is_empty() {
            RiskLevel::Deny
        } else if self.mode.eq_ignore_ascii_case("strict") {
            if high_risk.is_empty() {
                RiskLevel::Safe
            } else {
                RiskLevel::Deny
            }
        } else if !high_risk.is_empty() {
            RiskLevel::HighRisk
        } else {
            RiskLevel::Safe
        };

        let reasons = match level {
            RiskLevel::Deny => unique(deny),
            RiskLevel::HighRisk => unique(high_risk),
            RiskLevel::Safe => Vec::new(),
        };

        RiskReport { level, reasons }
    }

    fn touches_path_outside_cwd(&self, command: &str, cwd: &Path) -> bool {
        let lowered = command.to_lowercase();
        let writes = [
            " rm ", " mv ", " chmod ", " chown ", " cp ", " tee ", " mkdir ", " touch ",
        ];
        let padded = format!(" {lowered} ");
        if !writes.iter().any(|pattern| padded.contains(pattern)) {
            return false;
        }

        let cwd_text = cwd.display().to_string();
        command.contains("../")
            || command.contains("~/")
            || command.contains(" /")
            || (!cwd_text.is_empty() && command.contains('/') && !command.contains(&cwd_text))
    }

    fn targets_home_root(&self, command: &str) -> bool {
        let Some(home_dir) = &self.home_dir else {
            return false;
        };

        let home = home_dir.display().to_string();
        let lowered = command.to_lowercase();
        lowered.contains(&format!("rm -rf {home}")) || lowered.contains("rm -rf ~/")
    }

    fn redirects_to_regular_file(&self, command: &str) -> bool {
        REDIRECTION_RE.captures_iter(command).any(|captures| {
            let Some(target) = captures.name("target") else {
                return false;
            };

            let target = target.as_str().trim_matches(['"', '\'']).to_lowercase();
            !matches!(
                target.as_str(),
                "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/fd/1" | "/dev/fd/2"
            )
        })
    }
}

fn has_background_operator(input: &str) -> bool {
    let bytes = input.as_bytes();
    for (index, current) in bytes.iter().enumerate() {
        if *current != b'&' {
            continue;
        }
        let previous_is_amp = index > 0 && bytes[index - 1] == b'&';
        let next_is_amp = index + 1 < bytes.len() && bytes[index + 1] == b'&';
        if !previous_is_amp && !next_is_amp {
            return true;
        }
    }
    false
}

fn unique(items: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for item in items {
        if !deduped.contains(&item) {
            deduped.push(item);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PolicyEngine, PolicyRules, RiskLevel};

    fn engine() -> PolicyEngine {
        PolicyEngine::new(
            "tiered".to_string(),
            PolicyRules {
                denylist: vec!["mkfs".to_string()],
                confirmlist: vec!["sudo".to_string(), "rm".to_string()],
            },
        )
    }

    #[test]
    fn denies_eval_and_substitution() {
        let engine = engine();
        let report = engine.classify("eval $(cat payload.sh)", Path::new("."));
        assert_eq!(report.level, RiskLevel::Deny);
    }

    #[test]
    fn asks_confirmation_for_sudo() {
        let engine = engine();
        let report = engine.classify("sudo rm -rf ./target", Path::new("."));
        assert_eq!(report.level, RiskLevel::HighRisk);
    }

    #[test]
    fn allows_read_only_command() {
        let engine = engine();
        let report = engine.classify("du -sh ./* | sort -hr | head -n 5", Path::new("."));
        assert_eq!(report.level, RiskLevel::Safe);
    }

    #[test]
    fn does_not_deny_unknown_command_when_not_in_denylist() {
        let engine = engine();
        let report = engine.classify("python3 -c 'print(1)'", Path::new("."));
        assert_eq!(report.level, RiskLevel::Safe);
    }

    #[test]
    fn does_not_flag_dev_null_redirection_as_high_risk() {
        let engine = engine();
        let report = engine.classify(r#"du -sh "*/" 2>/dev/null | sort -hr"#, Path::new("."));
        assert_eq!(report.level, RiskLevel::Safe);
    }

    #[test]
    fn flags_regular_file_redirection_as_high_risk() {
        let engine = engine();
        let report = engine.classify("ls -la > out.txt", Path::new("."));
        assert_eq!(report.level, RiskLevel::HighRisk);
    }
}
