use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{
    config::HistoryConfig,
    executor::RunResult,
    policy::{RiskLevel, RiskReport},
    review::ReviewSnapshot,
};

pub struct HistoryStore {
    enabled: bool,
    path: PathBuf,
    max_entries: usize,
    redact_paths: bool,
    cwd: PathBuf,
    home_dir: Option<PathBuf>,
}

impl HistoryStore {
    pub fn new(config: &HistoryConfig, path: PathBuf, cwd: &Path) -> Result<Self> {
        Ok(Self {
            enabled: config.enabled,
            path,
            max_entries: config.max_entries.max(1),
            redact_paths: config.redact_paths,
            cwd: cwd.to_path_buf(),
            home_dir: dirs::home_dir(),
        })
    }

    pub fn append(&self, entry: HistoryEntry) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create history directory: {}", parent.display())
            })?;
        }

        let serialized = serde_json::to_string(&self.redact_entry(entry))
            .context("Failed to serialize history entry")?;

        let mut lines = if self.path.exists() {
            fs::read_to_string(&self.path)
                .with_context(|| format!("Failed to read history file: {}", self.path.display()))?
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        lines.push(serialized);
        if lines.len() > self.max_entries {
            let keep_from = lines.len() - self.max_entries;
            lines = lines.split_off(keep_from);
        }

        let payload = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };

        fs::write(&self.path, payload)
            .with_context(|| format!("Failed to write history file: {}", self.path.display()))?;
        Ok(())
    }

    fn redact_entry(&self, mut entry: HistoryEntry) -> HistoryEntry {
        if !self.redact_paths {
            return entry;
        }

        entry.goal = self.redact_text(&entry.goal);
        entry.feedback = entry
            .feedback
            .into_iter()
            .map(|item| self.redact_text(&item))
            .collect();
        entry.command = entry.command.map(|value| self.redact_text(&value));
        entry.summary = entry.summary.map(|value| self.redact_text(&value));
        entry.assumptions = entry
            .assumptions
            .into_iter()
            .map(|item| self.redact_text(&item))
            .collect();
        entry.risk_hints = entry
            .risk_hints
            .into_iter()
            .map(|item| self.redact_text(&item))
            .collect();
        entry
    }

    fn redact_text(&self, input: &str) -> String {
        let mut output = input.to_string();
        if let Some(home_dir) = &self.home_dir {
            output = output.replace(&home_dir.display().to_string(), "<HOME>");
        }
        output.replace(&self.cwd.display().to_string(), "<CWD>")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HistoryEntry {
    pub timestamp: DateTime<Utc>,
    pub status: String,
    pub goal: String,
    pub feedback: Vec<String>,
    pub command: Option<String>,
    pub summary: Option<String>,
    pub assumptions: Vec<String>,
    pub risk_hints: Vec<String>,
    pub risk_level: String,
    pub reasons: Vec<String>,
    pub exit_code: Option<i32>,
}

impl HistoryEntry {
    pub fn from_print_only(
        goal: &str,
        generated: &crate::llm::GenerationResult,
        risk: &RiskReport,
        feedback: &[String],
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            status: "print-only".to_string(),
            goal: goal.to_string(),
            feedback: feedback.to_vec(),
            command: Some(generated.command.clone()),
            summary: Some(generated.summary.clone()),
            assumptions: generated.assumptions.clone(),
            risk_hints: generated.risk_hints.clone(),
            risk_level: risk.level.to_string(),
            reasons: risk.reasons.clone(),
            exit_code: None,
        }
    }

    pub fn from_cancelled(goal: &str, snapshot: &ReviewSnapshot) -> Self {
        Self {
            timestamp: Utc::now(),
            status: "cancelled".to_string(),
            goal: goal.to_string(),
            feedback: snapshot.feedback_history.clone(),
            command: Some(snapshot.command.clone()),
            summary: Some(snapshot.summary.clone()),
            assumptions: snapshot.assumptions.clone(),
            risk_hints: snapshot.risk_hints.clone(),
            risk_level: snapshot.risk_level.to_string(),
            reasons: snapshot.risk_reasons.clone(),
            exit_code: None,
        }
    }

    pub fn from_execution(goal: &str, snapshot: &ReviewSnapshot, result: &RunResult) -> Self {
        Self {
            timestamp: Utc::now(),
            status: "executed".to_string(),
            goal: goal.to_string(),
            feedback: snapshot.feedback_history.clone(),
            command: Some(snapshot.command.clone()),
            summary: Some(snapshot.summary.clone()),
            assumptions: snapshot.assumptions.clone(),
            risk_hints: snapshot.risk_hints.clone(),
            risk_level: snapshot.risk_level.to_string(),
            reasons: snapshot.risk_reasons.clone(),
            exit_code: Some(result.exit_code),
        }
    }
}

impl From<RiskLevel> for String {
    fn from(value: RiskLevel) -> Self {
        value.to_string()
    }
}

impl HistoryConfig {
    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::Utc;

    use super::{HistoryEntry, HistoryStore};

    fn unique_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("atai-{name}-{}-{stamp}.jsonl", process::id()))
    }

    fn sample_entry(goal: &str) -> HistoryEntry {
        HistoryEntry {
            timestamp: Utc::now(),
            status: "executed".to_string(),
            goal: goal.to_string(),
            feedback: vec!["feedback".to_string()],
            command: Some("echo demo".to_string()),
            summary: Some("summary".to_string()),
            assumptions: vec!["assumption".to_string()],
            risk_hints: vec!["hint".to_string()],
            risk_level: "SAFE".to_string(),
            reasons: Vec::new(),
            exit_code: Some(0),
        }
    }

    #[test]
    fn redacts_home_and_cwd_in_history() {
        let path = unique_path("redact");
        let store = HistoryStore {
            enabled: true,
            path: path.clone(),
            max_entries: 5,
            redact_paths: true,
            cwd: PathBuf::from("/tmp/project"),
            home_dir: Some(PathBuf::from("/tmp/home-user")),
        };

        let entry = HistoryEntry {
            goal: "inspect files under /tmp/project and /tmp/home-user".to_string(),
            feedback: vec!["write output to /tmp/project/result.txt".to_string()],
            command: Some("ls /tmp/project && ls /tmp/home-user".to_string()),
            summary: Some("read /tmp/project".to_string()),
            assumptions: vec!["the directory is under /tmp/home-user".to_string()],
            risk_hints: vec!["may access /tmp/project".to_string()],
            ..sample_entry("ignored")
        };

        store.append(entry).expect("append");
        let written = fs::read_to_string(&path).expect("read history");

        assert!(written.contains("<CWD>"));
        assert!(written.contains("<HOME>"));
        assert!(!written.contains("/tmp/project"));
        assert!(!written.contains("/tmp/home-user"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn keeps_only_latest_entries() {
        let path = unique_path("max-entries");
        let store = HistoryStore {
            enabled: true,
            path: path.clone(),
            max_entries: 2,
            redact_paths: false,
            cwd: PathBuf::from("."),
            home_dir: None,
        };

        store.append(sample_entry("first")).expect("append first");
        store.append(sample_entry("second")).expect("append second");
        store.append(sample_entry("third")).expect("append third");

        let written = fs::read_to_string(&path).expect("read history");
        let lines = written.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(written.contains("\"goal\":\"second\""));
        assert!(written.contains("\"goal\":\"third\""));
        assert!(!written.contains("\"goal\":\"first\""));

        let _ = fs::remove_file(path);
    }
}
