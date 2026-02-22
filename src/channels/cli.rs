use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::core::config::{CliConfigOverrides, load_runtime_config};
use crate::core::context::build_messages;
use crate::core::error::{CrabClawError, Result};
use crate::core::input::resolve_prompt;
use crate::llm::api_types::ChatRequest;
use crate::llm::client::send_chat_request;

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
    #[arg(long = "system-prompt")]
    system_prompt: Option<String>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct InteractiveArgs {
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

#[derive(Debug, Args)]
struct ServeArgs {
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
    }
}

fn run_command(args: RunArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = CliConfigOverrides {
        api_key: args.api_key,
        api_base: args.api_base,
        model: args.model,
        system_prompt: args.system_prompt,
    };
    let config = load_runtime_config(&workspace, args.profile.as_deref(), &overrides)?;
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

    // Initialize tape store for session recording.
    let tape_dir = workspace.join(".crabclaw");
    let mut tape =
        crate::tape::store::TapeStore::open(&tape_dir, "default").map_err(CrabClawError::Io)?;
    tape.ensure_bootstrap_anchor().map_err(CrabClawError::Io)?;

    // Route input through the command router.
    let route = crate::core::router::route_user(&prompt, &mut tape, &workspace);

    if route.exit_requested {
        return Ok(());
    }

    if !route.immediate_output.is_empty() {
        println!("{}", route.immediate_output);
    }

    if !route.enter_model {
        return Ok(());
    }

    // Record user message to tape.
    tape.append_message("user", &route.model_prompt)
        .map_err(CrabClawError::Io)?;

    // Build multi-turn messages from tape context.
    let messages = build_messages(&tape, config.system_prompt.as_deref());

    let request = ChatRequest {
        model: config.model.clone(),
        messages,
        max_tokens: None,
        tools: None,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

    let response = rt.block_on(send_chat_request(&config, &request))?;

    if let Some(content) = response.assistant_content() {
        // Record assistant response to tape.
        tape.append_message("assistant", content)
            .map_err(CrabClawError::Io)?;
        println!("{content}");
    } else {
        eprintln!("warning: no response content from model");
    }

    Ok(())
}

fn interactive_command(args: InteractiveArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = CliConfigOverrides {
        api_key: args.api_key,
        api_base: args.api_base,
        model: args.model,
        system_prompt: args.system_prompt,
    };
    let config = load_runtime_config(&workspace, args.profile.as_deref(), &overrides)?;
    crate::channels::repl::run_interactive(&config, &workspace)
}

fn serve_command(args: ServeArgs) -> Result<()> {
    let workspace = std::env::current_dir().map_err(CrabClawError::Io)?;
    let overrides = CliConfigOverrides {
        api_key: args.api_key,
        api_base: args.api_base,
        model: args.model,
        system_prompt: args.system_prompt,
    };
    let config = load_runtime_config(&workspace, args.profile.as_deref(), &overrides)?;
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
