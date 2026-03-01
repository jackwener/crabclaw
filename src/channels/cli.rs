use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::core::config::{CliConfigOverrides, load_runtime_config};
use crate::core::error::{CrabClawError, Result};
use crate::core::input::resolve_prompt;

#[derive(Debug, Parser)]
#[command(
    name = "crabclaw",
    about = "Rust implementation baseline for bub/OpenClaw"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Execute a single prompt (one-shot or routed)
    Run(RunArgs),
    /// Start an interactive REPL session
    Interactive(InteractiveArgs),
    /// Start channel server (Telegram, etc.)
    Serve(ServeArgs),
    /// Manage OAuth authentication
    Auth(AuthArgs),
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    action: AuthAction,
}

#[derive(Debug, Subcommand)]
enum AuthAction {
    /// Login with your ChatGPT account via OAuth
    Login,
    /// Remove stored OAuth tokens
    Logout,
    /// Show current auth status
    Status,
}

/// Common CLI arguments shared across all subcommands.
#[derive(Debug, Args)]
struct CommonArgs {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long = "api-key")]
    api_key: Option<String>,
    #[arg(long = "api-base")]
    api_base: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long = "system-prompt")]
    system_prompt: Option<String>,
}

impl CommonArgs {
    fn to_overrides(&self) -> CliConfigOverrides {
        CliConfigOverrides {
            api_key: self.api_key.clone(),
            api_base: self.api_base.clone(),
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
            max_context_messages: None,
        }
    }
}

#[derive(Debug, Args)]
struct RunArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long = "prompt-file")]
    prompt_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct InteractiveArgs {
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[command(flatten)]
    common: CommonArgs,
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
        Commands::Interactive(args) => interactive_command(args),
        Commands::Serve(args) => serve_command(args),
        Commands::Auth(args) => auth_command(args),
    }
}

fn auth_command(args: AuthArgs) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

    match args.action {
        AuthAction::Login => {
            rt.block_on(crate::core::auth::login())?;
        }
        AuthAction::Logout => {
            crate::core::auth::clear_tokens()?;
            println!("✅ Logged out. OAuth tokens removed.");
        }
        AuthAction::Status => {
            crate::core::auth::status();
        }
    }
    Ok(())
}

fn run_command(args: RunArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = args.common.to_overrides();
    let config = load_runtime_config(&workspace, args.common.profile.as_deref(), &overrides)?;
    let prompt = resolve_prompt(args.prompt, args.prompt_file)?;

    if args.dry_run {
        let out = DryRunOutput {
            mode: "dry-run".to_string(),
            profile: config.profile.clone(),
            prompt,
            config: DryRunConfig {
                api_base: config.api_base.clone(),
                model: config.model.clone(),
                api_key_present: !config.api_key.trim().is_empty(),
            },
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let mut agent =
        crate::core::agent_loop::AgentLoop::open(&config, &workspace, "default", None, None)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

    let mut has_started_text = false;
    let result = rt.block_on(agent.handle_input_stream(&prompt, |token| {
        if !has_started_text {
            println!();
            has_started_text = true;
        }
        print!("{token}");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    }));

    if has_started_text {
        println!();
    }

    if result.exit_requested {
        return Ok(());
    }

    if let Some(output) = &result.immediate_output {
        println!("{output}");
    }

    if let Some(err) = &result.error {
        eprintln!("error: {err}");
    }

    Ok(())
}

fn interactive_command(args: InteractiveArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = args.common.to_overrides();
    let config = load_runtime_config(&workspace, args.common.profile.as_deref(), &overrides)?;
    crate::channels::repl::run_interactive(&config, &workspace)
}

fn serve_command(args: ServeArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = args.common.to_overrides();
    let config = load_runtime_config(&workspace, args.common.profile.as_deref(), &overrides)?;
    let config = Arc::new(config);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

    rt.block_on(async {
        let mut manager =
            crate::channels::manager::ChannelManager::new(Arc::clone(&config), &workspace);
        manager.run().await
    })
}
