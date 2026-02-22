use std::sync::Arc;

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MediaKind, MessageKind};
use tracing::{debug, info, warn};

use crate::channels::base::{Channel, ChannelResponse};
use crate::core::config::AppConfig;
use crate::core::context::build_messages;
use crate::core::router::route_user;
use crate::tape::store::TapeStore;

/// Telegram channel adapter using long polling.
///
/// Aligned with bub's `TelegramChannel`:
/// - Long polling for updates
/// - ACL via allow_from (user IDs/usernames) and allow_chats (chat IDs)
/// - Typing indicator during processing
/// - Routes messages through CrabClaw router + model pipeline
pub struct TelegramChannel {
    config: Arc<AppConfig>,
    workspace: std::path::PathBuf,
}

impl TelegramChannel {
    pub fn new(config: Arc<AppConfig>, workspace: std::path::PathBuf) -> Self {
        Self { config, workspace }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&mut self) -> crate::core::error::Result<()> {
        let token = self
            .config
            .telegram_token
            .as_ref()
            .ok_or_else(|| {
                crate::core::error::CrabClawError::Config("TELEGRAM_TOKEN not set".into())
            })?
            .clone();

        info!("telegram.start");

        let bot = Bot::new(&token);
        let config = Arc::clone(&self.config);
        let workspace = self.workspace.clone();

        let handler = Update::filter_message().endpoint(move |bot: Bot, msg: Message| {
            let config = Arc::clone(&config);
            let workspace = workspace.clone();
            async move {
                handle_message(bot, msg, config, &workspace).await;
                respond(())
            }
        });

        Dispatcher::builder(bot, handler)
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        Ok(())
    }

    async fn stop(&mut self) -> crate::core::error::Result<()> {
        info!("telegram.stop");
        Ok(())
    }
}

async fn handle_message(
    bot: Bot,
    msg: Message,
    config: Arc<AppConfig>,
    workspace: &std::path::Path,
) {
    // Extract text content from various message types
    let text = match &msg.kind {
        MessageKind::Common(common) => match &common.media_kind {
            MediaKind::Text(t) => t.text.clone(),
            MediaKind::Photo(p) => {
                let caption = p.caption.clone().unwrap_or_default();
                if caption.is_empty() {
                    "[Photo received]".to_string()
                } else {
                    format!("[Photo] {caption}")
                }
            }
            MediaKind::Document(d) => {
                let file_name = d
                    .document
                    .file_name
                    .clone()
                    .unwrap_or_else(|| "unnamed".to_string());
                let caption = d.caption.clone().unwrap_or_default();
                if caption.is_empty() {
                    format!("[Document: {file_name}]")
                } else {
                    format!("[Document: {file_name}] {caption}")
                }
            }
            MediaKind::Voice(_v) => "[Voice message received]".to_string(),
            MediaKind::Sticker(s) => {
                let emoji = s.sticker.emoji.clone().unwrap_or_default();
                format!("[Sticker: {emoji}]")
            }
            _ => {
                debug!("telegram.ignore unsupported media type");
                return;
            }
        },
        _ => return,
    };

    let chat_id = msg.chat.id;

    // ACL check
    if let Some(user) = msg.from.as_ref() {
        let allow_from = &config.telegram_allow_from;
        let allow_chats = &config.telegram_allow_chats;

        if !allow_from.is_empty() || !allow_chats.is_empty() {
            let user_id_str = user.id.0.to_string();
            let username = user.username.clone().unwrap_or_default();
            let chat_id_str = chat_id.0.to_string();

            let chat_ok = allow_chats.is_empty() || allow_chats.contains(&chat_id_str);
            let user_ok = allow_from.is_empty()
                || allow_from.contains(&user_id_str)
                || (!username.is_empty() && allow_from.contains(&username));

            if !chat_ok || !user_ok {
                warn!(
                    "telegram.acl.deny user_id={} username={} chat_id={}",
                    user_id_str, username, chat_id_str
                );
                let _ = bot.send_message(chat_id, "Access denied.").await;
                return;
            }
        }
    }

    let session_id = format!("telegram:{}", chat_id.0);
    info!(
        session_id = %session_id,
        text = %text,
        "telegram.inbound"
    );

    // Sustained typing indicator — sends every 4 seconds until processing completes
    let bot_clone = bot.clone();
    let typing_handle = tokio::spawn(async move {
        loop {
            let _ = bot_clone
                .send_chat_action(chat_id, ChatAction::Typing)
                .await;
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
    });

    // Process through CrabClaw router + model + tool calling
    let response = process_message(&text, &config, workspace, &session_id).await;

    // Stop typing indicator
    typing_handle.abort();

    if let Some(reply) = response.to_reply() {
        // Telegram has a 4096 char limit per message
        for chunk in split_message(&reply, 4096) {
            if let Err(e) = bot.send_message(chat_id, &chunk).await {
                warn!("telegram.send.error: {e}");
            }
        }
    }
}

/// Maximum number of tool calling iterations to prevent infinite loops.
const MAX_TOOL_ITERATIONS: usize = 5;

/// Process a message through the CrabClaw router + model pipeline.
/// Exposed as pub for integration testing — call this directly to test
/// end-to-end message handling without needing a real Telegram connection.
///
/// Supports tool calling: if the model returns tool_calls, this function
/// executes the tools and re-invokes the model with tool results, up to
/// MAX_TOOL_ITERATIONS times.
pub async fn process_message(
    text: &str,
    config: &AppConfig,
    workspace: &std::path::Path,
    session_id: &str,
) -> ChannelResponse {
    // Open or create tape for this session
    let tape_dir = workspace.join(".crabclaw");
    let tape_name = session_id.replace(':', "_");
    let tape = TapeStore::open(&tape_dir, &tape_name);

    let mut tape = match tape {
        Ok(t) => t,
        Err(e) => {
            warn!("telegram.tape.error: {e}");
            return ChannelResponse {
                error: Some(format!("tape error: {e}")),
                ..Default::default()
            };
        }
    };

    tape.ensure_bootstrap_anchor().ok();

    // Route the user input
    let route_result = route_user(text, &mut tape, workspace);

    // Record user message
    tape.append_message("user", text).ok();

    let mut response = ChannelResponse {
        immediate_output: if route_result.immediate_output.is_empty() {
            None
        } else {
            Some(route_result.immediate_output.clone())
        },
        ..Default::default()
    };

    // If we need the model, send the request (with tool calling loop)
    if route_result.enter_model {
        // Build tool definitions from the registry (builtins + skills)
        let mut registry = crate::tools::registry::builtin_registry();
        crate::tools::registry::register_skills(&mut registry, workspace);
        let tool_defs = crate::tools::registry::to_tool_definitions(&registry);
        let tools = if tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs)
        };

        let system_prompt =
            crate::core::context::build_system_prompt(config.system_prompt.as_deref(), workspace);
        let mut messages = build_messages(&tape, Some(&system_prompt));

        for iteration in 0..MAX_TOOL_ITERATIONS {
            let request = crate::llm::api_types::ChatRequest {
                model: config.model.clone(),
                messages: messages.clone(),
                max_tokens: None,
                tools: tools.clone(),
            };

            match crate::llm::client::send_chat_request(config, &request).await {
                Ok(chat_response) => {
                    // Check if model wants to call tools
                    if chat_response.has_tool_calls() {
                        if let Some(tool_calls) = chat_response.tool_calls() {
                            debug!(
                                iteration = iteration,
                                tool_count = tool_calls.len(),
                                "telegram.tool_calls"
                            );

                            // Append the assistant message with tool_calls to context
                            messages.push(
                                crate::llm::api_types::Message::assistant_with_tool_calls(
                                    tool_calls.to_vec(),
                                ),
                            );

                            // Execute each tool and append results
                            for tc in tool_calls {
                                let result = crate::tools::registry::execute_tool(
                                    &tc.function.name,
                                    &tc.function.arguments,
                                    &tape,
                                    workspace,
                                );
                                debug!(
                                    tool = %tc.function.name,
                                    result = %result,
                                    "telegram.tool_result"
                                );
                                messages
                                    .push(crate::llm::api_types::Message::tool(&tc.id, &result));
                            }
                            // Continue loop — re-call model with tool results
                            continue;
                        }
                    }

                    // No tool calls — we have the final response
                    if let Some(content) = chat_response.assistant_content() {
                        tape.append_message("assistant", content).ok();
                        response.assistant_output = Some(content.to_string());
                    }
                    break;
                }
                Err(e) => {
                    response.error = Some(format!("{e}"));
                    break;
                }
            }
        }
    }

    response
}

/// Split a long message into chunks that fit Telegram's per-message limit.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Try to split at a newline within limit
        let split_at = remaining[..max_len].rfind('\n').unwrap_or(max_len);

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_message() {
        let chunks = split_message("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_long_message() {
        let long = "a".repeat(5000);
        let chunks = split_message(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn split_at_newline() {
        // 4000 + '\n' + 200 = 4201 chars, exceeds 4096, splits at newline
        let text = format!("{}\n{}", "a".repeat(4000), "b".repeat(200));
        let chunks = split_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4000); // split at newline
        assert_eq!(chunks[1].len(), 200);
    }

    #[test]
    fn channel_response_to_reply_with_error() {
        let r = ChannelResponse {
            error: Some("something broke".to_string()),
            ..Default::default()
        };
        let reply = r.to_reply().unwrap();
        assert!(reply.contains("something broke"));
    }
}
