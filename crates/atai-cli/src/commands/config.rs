use std::path::Path;

use anyhow::Result;
use atai_core::{
    config::Config,
    resources::{AppPaths, RuntimeResources},
};

use crate::cli::ConfigCommand;

pub fn run(command: ConfigCommand, path: Option<&Path>) -> Result<()> {
    match command {
        ConfigCommand::Show => show(path),
        ConfigCommand::Init => init(path),
    }
}

fn show(path: Option<&Path>) -> Result<()> {
    let runtime_paths = AppPaths::from_config_path(path)?;
    let (config, path) = Config::read(path)?;
    println!(
        "# Resource root: {}\n# system prompt: {}\n# denylist: {}\n# confirmlist: {}\n# history: {}\n",
        runtime_paths.root_dir.display(),
        runtime_paths.prompt_path.display(),
        runtime_paths.denylist_path.display(),
        runtime_paths.confirmlist_path.display(),
        runtime_paths.history_path.display(),
    );
    println!("{}", config.render_for_display(&path));
    Ok(())
}

fn init(path: Option<&Path>) -> Result<()> {
    let report = RuntimeResources::init(path)?;
    if !report.created.is_empty() {
        println!("Created:");
        for item in &report.created {
            println!("- {}", item.display());
        }
    }
    if !report.skipped.is_empty() {
        println!("Already exists, skipped:");
        for item in &report.skipped {
            println!("- {}", item.display());
        }
    }
    Ok(())
}
