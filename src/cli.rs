use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::{CliConfigOverrides, load_runtime_config};
use crate::error::{CrabClawError, Result};
use crate::input::resolve_prompt;

#[derive(Debug, Parser)]
#[command(name = "crabclaw", about = "Rust implementation baseline for bub/OpenClaw")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long = "prompt-file")]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long = "api-key")]
    api_key: Option<String>,
    #[arg(long = "api-base")]
    api_base: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct DryRunOutput {
    mode: String,
    profile: String,
    prompt: String,
    config: DryRunConfig,
}

#[derive(Debug, Serialize)]
struct DryRunConfig {
    api_base: String,
    model: String,
    api_key_present: bool,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Run(args) => run_command(args),
    }
}

fn run_command(args: RunArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = CliConfigOverrides {
        api_key: args.api_key,
        api_base: args.api_base,
        model: args.model,
    };
    let config = load_runtime_config(&workspace, args.profile.as_deref(), &overrides)?;
    let prompt = resolve_prompt(args.prompt, args.prompt_file)?;

    if args.dry_run {
        let out = DryRunOutput {
            mode: "dry-run".to_string(),
            profile: config.profile,
            prompt,
            config: DryRunConfig {
                api_base: config.api_base,
                model: config.model,
                api_key_present: !config.api_key.trim().is_empty(),
            },
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Request execution pipeline is not implemented yet. Use --dry-run for validation.");
    Ok(())
}
