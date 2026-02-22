use std::sync::Arc;

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MediaKind, MessageKind};
use tracing::{debug, info, warn};

use crate::channel::{Channel, ChannelResponse};
use crate::config::AppConfig;
use crate::context::build_messages;
use crate::router::route_user;
use crate::tape::TapeStore;

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

    async fn start(&mut self) -> crate::error::Result<()> {
        let token = self
            .config
            .telegram_token
            .as_ref()
            .ok_or_else(|| {
                crate::error::CrabClawError::Config("BUB_TELEGRAM_TOKEN not set".into())
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

    async fn stop(&mut self) -> crate::error::Result<()> {
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
    // Extract text content
    let text = match &msg.kind {
        MessageKind::Common(common) => match &common.media_kind {
            MediaKind::Text(t) => t.text.clone(),
            _ => {
                debug!("telegram.ignore non-text message");
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

    // Send typing indicator
    let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;

    // Process through CrabClaw router
    let response = process_message(&text, &config, workspace, &session_id).await;

    if let Some(reply) = response.to_reply() {
        // Telegram has a 4096 char limit per message
        for chunk in split_message(&reply, 4096) {
            if let Err(e) = bot.send_message(chat_id, &chunk).await {
                warn!("telegram.send.error: {e}");
            }
        }
    }
}

/// Process a message through the CrabClaw router + model pipeline.
/// Exposed as pub for integration testing — call this directly to test
/// end-to-end message handling without needing a real Telegram connection.
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

    // If we need the model, send the request
    if route_result.enter_model {
        let messages = build_messages(&tape, config.system_prompt.as_deref());
        let request = crate::api_types::ChatRequest {
            model: config.model.clone(),
            messages,
            max_tokens: None,
        };

        match crate::client::send_chat_request(config, &request).await {
            Ok(chat_response) => {
                if let Some(content) = chat_response.assistant_content() {
                    tape.append_message("assistant", content).ok();
                    response.assistant_output = Some(content.to_string());
                }
            }
            Err(e) => {
                response.error = Some(format!("{e}"));
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
