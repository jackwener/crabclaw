use std::sync::Arc;

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MediaKind, MessageKind};
use tracing::{debug, info, warn};

use crate::channels::base::{Channel, ChannelResponse};
use crate::core::config::AppConfig;

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

/// Process a message through the CrabClaw router + model pipeline.
/// Exposed as pub for integration testing — call this directly to test
/// end-to-end message handling without needing a real Telegram connection.
///
/// Delegates to `AgentLoop::handle_input` which handles:
/// - Command routing
/// - Tool calling loop (up to 5 iterations)
/// - Tape recording
/// - System prompt building
pub async fn process_message(
    text: &str,
    config: &AppConfig,
    workspace: &std::path::Path,
    session_id: &str,
) -> ChannelResponse {
    let mut agent = match crate::core::agent_loop::AgentLoop::open(config, workspace, session_id) {
        Ok(a) => a,
        Err(e) => {
            warn!("telegram.agent_loop.error: {e}");
            return ChannelResponse {
                error: Some(format!("{e}")),
                ..Default::default()
            };
        }
    };

    let result = agent.handle_input(text).await;

    ChannelResponse {
        immediate_output: result.immediate_output,
        assistant_output: result.assistant_output,
        error: result.error,
    }
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

        // Find a safe UTF-8 boundary at or before max_len
        let safe_end = remaining
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max_len)
            .last()
            .unwrap_or(max_len.min(remaining.len()));

        // Try to split at a newline within the safe range
        let split_at = remaining[..safe_end].rfind('\n').unwrap_or(safe_end);

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
