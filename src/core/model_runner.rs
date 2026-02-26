//! Unified model turn runner with tool-calling loop.
//!
//! Replaces the inlined tool-calling loops in process_message, run_interactive,
//! and run_command. Channels call `run_turn` (non-streaming) or
//! `run_turn_stream` (streaming with callback) for each model turn.

use std::path::Path;

use tracing::{debug, info, instrument};

use crate::core::config::AppConfig;
use crate::llm::api_types::{
    ChatRequest, Message, StreamChunk, ToolCall, ToolCallFunction, ToolDefinition,
};
use crate::tape::store::TapeStore;

/// Default maximum tool-calling rounds per turn.
const DEFAULT_MAX_TOOL_ITERATIONS: usize = 5;

/// Result of a single model turn (may include multiple tool-call rounds).
#[derive(Debug, Default)]
pub struct ModelTurnResult {
    /// Model's final text response (after all tool rounds).
    pub assistant_text: String,
    /// Total tool-calling rounds executed.
    pub tool_rounds: usize,
    /// Error if any occurred during the turn.
    pub error: Option<String>,
}

/// Unified model turn runner with tool-calling loop.
///
/// Encapsulates the shared logic of:
/// 1. Send request to LLM
/// 2. If model returns tool_calls → execute tools → re-call model
/// 3. Repeat up to `max_tool_iterations` times
/// 4. Return final assistant text
pub struct ModelRunner<'a> {
    config: &'a AppConfig,
    workspace: &'a Path,
    max_tool_iterations: usize,
}

impl<'a> ModelRunner<'a> {
    /// Create a new ModelRunner.
    pub fn new(config: &'a AppConfig, workspace: &'a Path) -> Self {
        Self {
            config,
            workspace,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
        }
    }

    /// Set the maximum number of tool-calling iterations.
    #[allow(dead_code)]
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_tool_iterations = max;
        self
    }

    /// Run a **non-streaming** model turn with tool calling loop.
    ///
    /// This is the async path used by Telegram and test harness.
    /// Returns the final assistant text after all tool rounds.
    #[instrument(skip_all, fields(model = %self.config.model, msg_count = messages.len()))]
    pub async fn run_turn(
        &self,
        messages: &mut Vec<Message>,
        tools: Option<&[ToolDefinition]>,
        tape: &TapeStore,
    ) -> ModelTurnResult {
        let mut result = ModelTurnResult::default();

        let tools_vec = tools.map(|t| t.to_vec());

        for iteration in 0..self.max_tool_iterations {
            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                max_tokens: None,
                tools: tools_vec.clone(),
            };

            match crate::llm::client::send_chat_request(self.config, &request).await {
                Ok(chat_response) => {
                    // Check if model wants to call tools
                    if let Some(tool_calls) = chat_response.tool_calls() {
                        info!(
                            iteration = iteration,
                            tool_count = tool_calls.len(),
                            "model_runner.tool_calls"
                        );

                        // Append the assistant message with tool_calls to context
                        messages.push(Message::assistant_with_tool_calls(tool_calls.to_vec()));

                        // Execute each tool and append results
                        for tc in tool_calls {
                            let tool_result = crate::tools::registry::execute_tool(
                                &tc.function.name,
                                &tc.function.arguments,
                                tape,
                                self.workspace,
                            );
                            debug!(
                                tool = %tc.function.name,
                                result_len = tool_result.len(),
                                "model_runner.tool_result"
                            );
                            messages.push(Message::tool(&tc.id, &tool_result));
                        }

                        result.tool_rounds += 1;
                        continue;
                    }

                    // No tool calls — we have the final response
                    if let Some(content) = chat_response.assistant_content() {
                        result.assistant_text = content.to_string();
                    }
                    break;
                }
                Err(e) => {
                    result.error = Some(format!("{e}"));
                    break;
                }
            }
        }

        result
    }

    /// Run a **streaming** model turn with tool calling loop.
    ///
    /// Used by CLI and REPL. Calls `on_token` for each streamed text chunk.
    /// After all tool rounds, returns the final result.
    #[instrument(skip_all, fields(model = %self.config.model, msg_count = messages.len()))]
    pub async fn run_turn_stream<F>(
        &self,
        messages: &mut Vec<Message>,
        tools: Option<&[ToolDefinition]>,
        tape: &TapeStore,
        mut on_token: F,
    ) -> ModelTurnResult
    where
        F: FnMut(&str),
    {
        let mut result = ModelTurnResult::default();
        let tools_vec = tools.map(|t| t.to_vec());

        for iteration in 0..self.max_tool_iterations {
            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                max_tokens: None,
                tools: tools_vec.clone(),
            };

            let rx_result =
                crate::llm::client::send_chat_request_stream(self.config, &request).await;

            match rx_result {
                Ok(mut rx) => {
                    let mut full_content = String::new();
                    let mut tool_calls = Vec::<ToolCall>::new();

                    while let Some(chunk_res) = rx.recv().await {
                        match chunk_res {
                            Ok(chunk) => match chunk {
                                StreamChunk::Content(text) => {
                                    on_token(&text);
                                    full_content.push_str(&text);
                                }
                                StreamChunk::ToolCallStart { index, id, name } => {
                                    if tool_calls.len() <= index {
                                        tool_calls.resize(
                                            index + 1,
                                            ToolCall {
                                                id: id.clone(),
                                                call_type: "function".to_string(),
                                                function: ToolCallFunction {
                                                    name: name.clone(),
                                                    arguments: String::new(),
                                                },
                                            },
                                        );
                                    } else {
                                        tool_calls[index].id.clone_from(&id);
                                        tool_calls[index].function.name.clone_from(&name);
                                    }
                                }
                                StreamChunk::ToolCallArgument { index, text } => {
                                    if index < tool_calls.len() {
                                        tool_calls[index].function.arguments.push_str(&text);
                                    }
                                }
                                StreamChunk::Done => {
                                    break;
                                }
                            },
                            Err(e) => {
                                result.error = Some(format!("{e}"));
                                return result;
                            }
                        }
                    }

                    // If we got tool calls, execute them and loop
                    if !tool_calls.is_empty() {
                        info!(
                            iteration = iteration,
                            tool_count = tool_calls.len(),
                            "model_runner.stream.tool_calls"
                        );

                        messages.push(Message::assistant_with_tool_calls(tool_calls.clone()));

                        for tc in &tool_calls {
                            let tool_result = crate::tools::registry::execute_tool(
                                &tc.function.name,
                                &tc.function.arguments,
                                tape,
                                self.workspace,
                            );
                            debug!(
                                tool = %tc.function.name,
                                result_len = tool_result.len(),
                                "model_runner.stream.tool_result"
                            );
                            messages.push(Message::tool(&tc.id, &tool_result));
                        }

                        result.tool_rounds += 1;
                        continue;
                    }

                    // No tool calls — we have the final response
                    result.assistant_text = full_content;
                    break;
                }
                Err(e) => {
                    result.error = Some(format!("{e}"));
                    break;
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> AppConfig {
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
    fn model_turn_result_default_is_empty() {
        let result = ModelTurnResult::default();
        assert!(result.assistant_text.is_empty());
        assert_eq!(result.tool_rounds, 0);
        assert!(result.error.is_none());
    }

    #[test]
    fn model_runner_default_max_iterations() {
        let config = make_test_config();
        let workspace = Path::new("/tmp");
        let runner = ModelRunner::new(&config, workspace);
        assert_eq!(runner.max_tool_iterations, DEFAULT_MAX_TOOL_ITERATIONS);
    }

    #[test]
    fn model_runner_with_max_iterations() {
        let config = make_test_config();
        let workspace = Path::new("/tmp");
        let runner = ModelRunner::new(&config, workspace).with_max_iterations(3);
        assert_eq!(runner.max_tool_iterations, 3);
    }
}
