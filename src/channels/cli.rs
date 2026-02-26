use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::core::config::{CliConfigOverrides, load_runtime_config};
use crate::core::context::build_messages;
use crate::core::error::{CrabClawError, Result};
use crate::core::input::resolve_prompt;
use crate::llm::api_types::ChatRequest;

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
    }
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
    let system_prompt =
        crate::core::context::build_system_prompt(config.system_prompt.as_deref(), &workspace);
    let messages = build_messages(&tape, Some(&system_prompt), config.max_context_messages);

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

    let mut rx = rt.block_on(crate::llm::client::send_chat_request_stream(
        &config, &request,
    ))?;
    let mut full_content = String::new();
    let mut has_started_text = false;

    rt.block_on(async {
        while let Some(chunk_res) = rx.recv().await {
            match chunk_res {
                Ok(chunk) => match chunk {
                    crate::llm::api_types::StreamChunk::Content(text) => {
                        if !has_started_text {
                            println!();
                            has_started_text = true;
                        }
                        print!("{text}");
                        std::io::Write::flush(&mut std::io::stdout()).unwrap();
                        full_content.push_str(&text);
                    }
                    crate::llm::api_types::StreamChunk::Done => {
                        if has_started_text {
                            println!();
                        }
                        break;
                    }
                    _ => {} // Ignore tool calls in single-run mode for now
                },
                Err(e) => {
                    eprintln!("\nerror: {e}");
                    break;
                }
            }
        }
    });

    if !full_content.is_empty() {
        // Record assistant response to tape.
        tape.append_message("assistant", &full_content)
            .map_err(CrabClawError::Io)?;
    } else {
        eprintln!("warning: no response content from model");
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
