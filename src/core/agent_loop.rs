//! Unified agent loop for all channels.
//!
//! Each channel creates one `AgentLoop` per session and calls `handle_input()`
//! for each user message. The loop owns tape, tool_view, and model_runner,
//! eliminating the duplicated logic across telegram, cli, and repl.

use std::path::Path;

use tracing::{debug, instrument, warn};

use crate::core::config::AppConfig;
use crate::core::context::{build_messages, build_system_prompt};
use crate::core::error::{CrabClawError, Result};
use crate::core::model_runner::{ModelRunner, ModelTurnResult};
use crate::core::router::route_user;
use crate::tape::store::TapeStore;
use crate::tools::progressive::ProgressiveToolView;
use crate::tools::registry::ToolContext;
use crate::tools::schedule::Notifier;

/// Output from one agent loop turn.
#[derive(Debug, Default)]
pub struct LoopResult {
    /// Immediate output from command routing (e.g. `,help` output).
    pub immediate_output: Option<String>,
    /// Model's final response text.
    pub assistant_output: Option<String>,
    /// Whether an exit was requested (e.g. `,quit`).
    pub exit_requested: bool,
    /// Number of tool-calling rounds executed.
    pub tool_rounds: usize,
    /// Error message if any.
    pub error: Option<String>,
}

impl LoopResult {
    /// Combine immediate_output and assistant_output into a single reply.
    ///
    /// Used by channels that want a single reply string.
    pub fn to_reply(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(imm) = self.immediate_output.as_ref().filter(|s| !s.is_empty()) {
            parts.push(imm.clone());
        }
        if let Some(asst) = self.assistant_output.as_ref().filter(|s| !s.is_empty()) {
            parts.push(asst.clone());
        }
        if let Some(err) = &self.error {
            parts.push(format!("Error: {err}"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

/// Deterministic single-session agent loop.
///
/// Each channel creates one `AgentLoop` per session and calls `handle_input()`
/// or `handle_input_stream()` for each user message.
///
/// Replaces the duplicated ~100-line agent loop logic in:
/// - `telegram::process_message`
/// - `repl::run_interactive`
/// - `cli::run_command`
pub struct AgentLoop<'a> {
    config: &'a AppConfig,
    workspace: &'a Path,
    tape: TapeStore,
    tool_view: ProgressiveToolView,
    tool_ctx: ToolContext,
}

impl<'a> AgentLoop<'a> {
    /// Create a new agent loop for a session.
    ///
    /// Opens or creates the tape file for `session_id`.
    /// `notifier` is an optional callback for delivering notifications
    /// (e.g. schedule reminders) back to the originating channel.
    /// `agent_runner` is an optional async callback for running the
    /// full agent pipeline on schedule fire (agent-mode jobs).
    pub fn open(
        config: &'a AppConfig,
        workspace: &'a Path,
        session_id: &str,
        notifier: Option<Notifier>,
        agent_runner: Option<crate::tools::schedule::AgentRunner>,
    ) -> Result<Self> {
        let tape_dir = workspace.join(".crabclaw");
        let tape_name = session_id.replace(':', "_");
        let tape = TapeStore::open(&tape_dir, &tape_name).map_err(CrabClawError::Io)?;

        // Build tool registry with builtins + workspace skills
        let mut registry = crate::tools::registry::builtin_registry();
        crate::tools::registry::register_skills(&mut registry, workspace);

        let tool_view = ProgressiveToolView::new(registry);

        let tool_ctx = ToolContext {
            notifier,
            agent_runner,
        };

        let mut loop_instance = Self {
            config,
            workspace,
            tape,
            tool_view,
            tool_ctx,
        };

        loop_instance
            .tape
            .ensure_bootstrap_anchor()
            .map_err(CrabClawError::Io)?;

        Ok(loop_instance)
    }

    /// Handle one user input message (**non-streaming**, for Telegram / tests).
    ///
    /// Routes input through the command router, and if the model is needed,
    /// calls the model with the tool-calling loop.
    #[instrument(skip_all, fields(input_len = text.len()))]
    pub async fn handle_input(&mut self, text: &str) -> LoopResult {
        let mut result = LoopResult::default();

        // 1. Route user input
        let route = route_user(text, &mut self.tape, self.workspace);

        if route.exit_requested {
            result.exit_requested = true;
            return result;
        }

        if !route.immediate_output.is_empty() {
            result.immediate_output = Some(route.immediate_output.clone());
        }

        if !route.enter_model {
            return result;
        }

        // 2. Record user message to tape
        if let Err(e) = self.tape.append_message("user", &route.model_prompt) {
            warn!("agent_loop.tape.write.error: {e}");
        }

        // 3. Build tool definitions from progressive view
        let tool_defs = self.tool_view.tool_definitions();
        let tools = if tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs)
        };

        // 4. Build system prompt and messages from tape context
        let system_prompt =
            build_system_prompt(self.config.system_prompt.as_deref(), self.workspace);
        let mut messages = build_messages(
            &self.tape,
            Some(&system_prompt),
            self.config.max_context_messages,
        );

        debug!(message_count = messages.len(), "agent_loop.model_request");

        // 5. Run model turn with tool calling loop
        let runner = ModelRunner::new(self.config, self.workspace);
        let turn_result = runner
            .run_turn(&mut messages, tools.as_deref(), &self.tape, &self.tool_ctx)
            .await;

        // 6. Process result
        self.process_turn_result(&turn_result, &mut result);

        result
    }

    /// Handle one user input message (**streaming**, for CLI / REPL).
    ///
    /// `on_token` is called for each streamed text chunk from the model.
    #[instrument(skip_all, fields(input_len = text.len()))]
    pub async fn handle_input_stream<F>(&mut self, text: &str, on_token: F) -> LoopResult
    where
        F: FnMut(&str),
    {
        let mut result = LoopResult::default();

        // 1. Route user input
        let route = route_user(text, &mut self.tape, self.workspace);

        if route.exit_requested {
            result.exit_requested = true;
            return result;
        }

        if !route.immediate_output.is_empty() {
            result.immediate_output = Some(route.immediate_output.clone());
        }

        if !route.enter_model {
            return result;
        }

        // 2. Record user message to tape
        if let Err(e) = self.tape.append_message("user", &route.model_prompt) {
            warn!("agent_loop.tape.write.error: {e}");
        }

        // 3. Build tool definitions from progressive view
        let tool_defs = self.tool_view.tool_definitions();
        let tools = if tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs)
        };

        // 4. Build system prompt and messages from tape context
        let system_prompt =
            build_system_prompt(self.config.system_prompt.as_deref(), self.workspace);
        let mut messages = build_messages(
            &self.tape,
            Some(&system_prompt),
            self.config.max_context_messages,
        );

        debug!(message_count = messages.len(), "agent_loop.stream_request");

        // 5. Run streaming model turn with tool calling loop
        let runner = ModelRunner::new(self.config, self.workspace);
        let turn_result = runner
            .run_turn_stream(
                &mut messages,
                tools.as_deref(),
                &self.tape,
                &self.tool_ctx,
                on_token,
            )
            .await;

        // 6. Process result
        self.process_turn_result(&turn_result, &mut result);

        result
    }

    /// Process the model turn result: record to tape and populate LoopResult.
    fn process_turn_result(&mut self, turn: &ModelTurnResult, result: &mut LoopResult) {
        result.tool_rounds = turn.tool_rounds;

        if let Some(err) = &turn.error {
            result.error = Some(err.clone());
        }

        if !turn.assistant_text.is_empty() {
            // Activate progressive hints from model output
            let newly_expanded = self.tool_view.activate_hints(&turn.assistant_text);
            if !newly_expanded.is_empty() {
                debug!(tools = ?newly_expanded, "agent_loop.hints_activated");
            }

            // Route assistant output through command detection
            let assistant_route = crate::core::router::route_assistant(
                &turn.assistant_text,
                &mut self.tape,
                self.workspace,
            );

            if assistant_route.has_commands() {
                debug!(
                    commands = assistant_route.command_blocks.len(),
                    "agent_loop.assistant_commands_executed"
                );

                // Record the full assistant text first
                if let Err(e) = self.tape.append_message("assistant", &turn.assistant_text) {
                    warn!("agent_loop.tape.write.error: {e}");
                }

                // Record command results as a separate event
                if let Err(e) = self.tape.append_event(
                    "assistant_command_results",
                    serde_json::json!({
                        "blocks": assistant_route.command_blocks,
                    }),
                ) {
                    warn!("agent_loop.tape.write.error: {e}");
                }

                // Set visible text as output (commands stripped)
                if !assistant_route.visible_text.is_empty() {
                    result.assistant_output = Some(assistant_route.visible_text);
                }

                if assistant_route.exit_requested {
                    result.exit_requested = true;
                }
            } else {
                // No commands — record and return as-is
                if let Err(e) = self.tape.append_message("assistant", &turn.assistant_text) {
                    warn!("agent_loop.tape.write.error: {e}");
                }
                result.assistant_output = Some(turn.assistant_text.clone());
            }
        }
    }

    /// Access the tape store (for external recording or inspection).
    pub fn tape(&self) -> &TapeStore {
        &self.tape
    }

    /// Mutable access to the tape store.
    pub fn tape_mut(&mut self) -> &mut TapeStore {
        &mut self.tape
    }

    /// Reset the session tape.
    pub fn reset_tape(&mut self) -> Result<()> {
        self.tape.reset(false).map_err(CrabClawError::Io)?;
        self.tape
            .ensure_bootstrap_anchor()
            .map_err(CrabClawError::Io)?;
        self.tool_view.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_config() -> AppConfig {
        AppConfig {
            profile: String::new(),
            api_key: String::new(),
            api_base: String::new(),
            model: String::new(),
            system_prompt: None,
            telegram_token: None,
            telegram_allow_from: Vec::new(),
            telegram_allow_chats: Vec::new(),
            telegram_proxy: None,
            max_context_messages: 50,
        }
    }

    #[test]
    fn agent_loop_opens_and_creates_tape() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let loop_ = AgentLoop::open(&config, dir.path(), "test_session", None, None);
        assert!(loop_.is_ok());
        // Tape file should exist
        let tape_path = dir.path().join(".crabclaw").join("test_session.jsonl");
        assert!(tape_path.exists());
    }

    #[test]
    fn agent_loop_replaces_colons_in_session_id() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let loop_ = AgentLoop::open(&config, dir.path(), "telegram:12345", None, None);
        assert!(loop_.is_ok());
        let tape_path = dir.path().join(".crabclaw").join("telegram_12345.jsonl");
        assert!(tape_path.exists());
    }

    #[tokio::test]
    async fn handle_input_empty_returns_no_output() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let mut loop_ = AgentLoop::open(&config, dir.path(), "test", None, None).unwrap();
        let result = loop_.handle_input("").await;
        assert!(result.immediate_output.is_none());
        assert!(result.assistant_output.is_none());
        assert!(!result.exit_requested);
    }

    #[tokio::test]
    async fn handle_input_help_command() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let mut loop_ = AgentLoop::open(&config, dir.path(), "test", None, None).unwrap();
        let result = loop_.handle_input(",help").await;
        assert!(result.immediate_output.is_some());
        assert!(result.assistant_output.is_none());
        assert!(!result.exit_requested);
    }

    #[tokio::test]
    async fn handle_input_quit_exits() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let mut loop_ = AgentLoop::open(&config, dir.path(), "test", None, None).unwrap();
        let result = loop_.handle_input(",quit").await;
        assert!(result.exit_requested);
    }

    #[test]
    fn reset_tape_clears_and_re_bootstraps() {
        let dir = tempdir().unwrap();
        let config = test_config();
        let mut loop_ = AgentLoop::open(&config, dir.path(), "test", None, None).unwrap();
        loop_.tape_mut().append_message("user", "hello").unwrap();
        assert!(loop_.reset_tape().is_ok());
        // After reset, entries should be minimal (just bootstrap anchor)
        let entries = loop_.tape().entries();
        assert!(entries.len() <= 1);
    }

    #[test]
    fn loop_result_to_reply_combines_parts() {
        let result = LoopResult {
            immediate_output: Some("hello".to_string()),
            assistant_output: Some("world".to_string()),
            ..Default::default()
        };
        let reply = result.to_reply().unwrap();
        assert!(reply.contains("hello"));
        assert!(reply.contains("world"));
    }

    #[test]
    fn loop_result_to_reply_none_when_empty() {
        let result = LoopResult::default();
        assert!(result.to_reply().is_none());
    }
}
