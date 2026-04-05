use std::{env, process};

use anyhow::{Context, Result};
use atai_core::{
    executor::{Executor, RunResult},
    history::{HistoryEntry, HistoryStore},
    llm::{GenerationResult, LlmClient},
    policy::{PolicyEngine, PolicyRules, RiskReport},
    resources::RuntimeResources,
};
use atai_tui::SessionOutcome;

use crate::{
    cli::{Action, Args},
    commands,
};

pub async fn run() -> Result<()> {
    let args = Args::parse()?;
    let Args {
        config: config_path,
        shell,
        print_only,
        action,
    } = args;

    match action {
        Action::Version => commands::version::run(),
        Action::Config(command) => commands::config::run(command, config_path.as_deref())?,
        Action::Prompt(goal) => {
            run_prompt(goal, config_path.as_deref(), shell, print_only).await?;
        }
    }

    Ok(())
}

async fn run_prompt(
    goal: String,
    config_path: Option<&std::path::Path>,
    shell_override: Option<std::path::PathBuf>,
    print_only: bool,
) -> Result<()> {
    let cwd = env::current_dir().context("Failed to read the current working directory")?;

    let runtime = RuntimeResources::load(config_path)?;
    let mut config = runtime.config;
    if let Some(shell) = shell_override {
        config.execution.shell = shell.display().to_string();
    }

    let llm_client = LlmClient::new(&config.model, runtime.system_prompt)?;
    let policy_engine = PolicyEngine::new(
        config.safety.mode.clone(),
        PolicyRules {
            denylist: runtime.denylist,
            confirmlist: runtime.confirmlist,
        },
    );
    let history_store = HistoryStore::new(&config.history, runtime.paths.history_path, &cwd)?;

    if print_only {
        let generated = llm_client.generate_command(&goal, &[]).await?;
        let risk = policy_engine.classify(&generated.command, &cwd);
        print_candidate(&generated, &risk);
        history_store.append(HistoryEntry::from_print_only(&goal, &generated, &risk, &[]))?;
        return Ok(());
    }

    match atai_tui::run_review_session(&goal, &llm_client, &policy_engine, &cwd).await? {
        SessionOutcome::Cancelled(snapshot) => {
            if let Some(snapshot) = snapshot {
                history_store.append(HistoryEntry::from_cancelled(&goal, &snapshot))?;
            }
            Ok(())
        }
        SessionOutcome::Approved(snapshot) => {
            println!("Executing command:\n{}\n", snapshot.command);
            let run_result =
                Executor::run(&config.execution.shell_path(), &snapshot.command, &cwd)?;
            print_run_result(&run_result);
            history_store.append(HistoryEntry::from_execution(&goal, &snapshot, &run_result))?;
            if run_result.exit_code != 0 {
                process::exit(run_result.exit_code);
            }
            Ok(())
        }
    }
}

fn print_candidate(generated: &GenerationResult, risk: &RiskReport) {
    println!("Candidate command:\n{}\n", generated.command);
    println!("Summary:\n{}\n", generated.summary);

    if !generated.assumptions.is_empty() {
        println!("Assumptions:");
        for item in &generated.assumptions {
            println!("- {item}");
        }
        println!();
    }

    println!("Risk level: {}", risk.level);
    if !risk.reasons.is_empty() {
        println!("Risk reasons:");
        for item in &risk.reasons {
            println!("- {item}");
        }
        println!();
    }
}

fn print_run_result(result: &RunResult) {
    if !result.stdout.trim().is_empty() {
        println!("Stdout:\n{}", result.stdout);
    }

    if !result.stderr.trim().is_empty() {
        eprintln!("Stderr:\n{}", result.stderr);
    }

    println!(
        "Execution finished: exit code = {}, duration = {} ms",
        result.exit_code, result.duration_ms
    );
}
