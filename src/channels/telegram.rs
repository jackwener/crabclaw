use std::sync::Arc;

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MediaKind, MessageKind, ParseMode};
use tracing::{debug, info, warn};

use crate::channels::base::{Channel, ChannelResponse};
use crate::channels::telegram_notify::get_or_create_notifier_sender;
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
        let user_id_str = user.id.0.to_string();
        let username = user.username.as_deref();
        let chat_id_str = chat_id.0.to_string();
        if !acl_allows(
            &config.telegram_allow_from,
            &config.telegram_allow_chats,
            &user_id_str,
            username,
            &chat_id_str,
        ) {
            warn!(
                "telegram.acl.deny user_id={} username={} chat_id={}",
                user_id_str,
                username.unwrap_or_default(),
                chat_id_str
            );
            let _ = bot.send_message(chat_id, "Access denied.").await;
            return;
        }
    }

    // Build per-session notifier for schedule jobs (Bub-style context-bound callback)
    let notifier: Option<crate::tools::schedule::Notifier> = {
        let tg_token = config.telegram_token.clone().unwrap_or_default();
        let tg_chat_id = chat_id.0;
        let sender = get_or_create_notifier_sender(&tg_token, tg_chat_id).await;
        Some(std::sync::Arc::new(move |text: String| {
            if sender.send(text).is_err() {
                warn!(chat_id = tg_chat_id, "telegram.notifier.sender_closed");
            }
        }))
    };

    // Build per-session agent runner for scheduled agent-mode jobs.
    // When the job fires, this closure runs the full agent pipeline
    // (LLM + tools like web.fetch) and sends the result to Telegram.
    let agent_runner: Option<crate::tools::schedule::AgentRunner> = {
        let run_config = config.clone();
        let run_workspace = workspace.to_path_buf();
        let run_session = format!("telegram:{}", chat_id.0);
        let tg_token = config.telegram_token.clone().unwrap_or_default();
        let tg_chat_id = chat_id.0;
        Some(std::sync::Arc::new(move |prompt: String| {
            let config = run_config.clone();
            let workspace = run_workspace.clone();
            let session_id = run_session.clone();
            let token = tg_token.clone();
            let chat = tg_chat_id;
            Box::pin(async move {
                info!(
                    prompt = %prompt,
                    session_id = %session_id,
                    "schedule.agent_runner: starting agent execution"
                );

                // Run the full agent pipeline with the prompt
                let response =
                    process_message(&prompt, &config, &workspace, &session_id, None, None).await;

                // Deliver the result to the Telegram chat
                match response.to_reply() {
                    Some(reply) => {
                        info!(
                            reply_len = reply.len(),
                            "schedule.agent_runner: delivering result to telegram"
                        );
                        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
                        let client = reqwest::Client::new();
                        for chunk in split_message(&reply, 4096) {
                            let html = markdown_to_telegram_html(&chunk);
                            match client
                                .post(&url)
                                .json(&serde_json::json!({
                                    "chat_id": chat,
                                    "text": html,
                                    "parse_mode": "HTML",
                                }))
                                .send()
                                .await
                            {
                                Ok(resp) => {
                                    if !resp.status().is_success() {
                                        warn!(
                                            status = %resp.status(),
                                            "schedule.agent_runner: telegram sendMessage failed"
                                        );
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        error = %e,
                                        "schedule.agent_runner: telegram sendMessage error"
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        warn!("schedule.agent_runner: process_message returned empty response");
                    }
                }
            })
        }))
    };

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
    let response = process_message(
        &text,
        &config,
        workspace,
        &session_id,
        notifier,
        agent_runner,
    )
    .await;

    // Stop typing indicator
    typing_handle.abort();

    if let Some(reply) = response.to_reply() {
        // Telegram has a 4096 char limit per message.
        // Convert each chunk to HTML independently so tags aren't split across messages.
        for chunk in split_message(&reply, 4096) {
            let html = markdown_to_telegram_html(&chunk);
            let send_result = bot
                .send_message(chat_id, &html)
                .parse_mode(ParseMode::Html)
                .await;

            if let Err(e) = send_result {
                // Fallback: send as plain text if HTML parsing fails
                warn!("telegram.send.html_error: {e} — retrying without parse_mode");
                if let Err(e2) = bot.send_message(chat_id, &chunk).await {
                    warn!("telegram.send.plain_error: {e2}");
                }
            }
        }
    }
}

fn acl_allows(
    allow_from: &[String],
    allow_chats: &[String],
    user_id: &str,
    username: Option<&str>,
    chat_id: &str,
) -> bool {
    if allow_from.is_empty() && allow_chats.is_empty() {
        return true;
    }

    let chat_ok = allow_chats.is_empty() || allow_chats.iter().any(|c| c == chat_id);
    let user_ok = allow_from.is_empty()
        || allow_from.iter().any(|u| u == user_id)
        || username
            .filter(|u| !u.is_empty())
            .is_some_and(|u| allow_from.iter().any(|v| v == u));

    chat_ok && user_ok
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
    notifier: Option<crate::tools::schedule::Notifier>,
    agent_runner: Option<crate::tools::schedule::AgentRunner>,
) -> ChannelResponse {
    let mut agent = match crate::core::agent_loop::AgentLoop::open(
        config,
        workspace,
        session_id,
        notifier,
        agent_runner,
    ) {
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
    if max_len == 0 {
        return Vec::new();
    }

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
        let mut split_at = remaining[..safe_end].rfind('\n').unwrap_or(safe_end);
        if split_at == 0 {
            // Ensure forward progress even when max_len is smaller than the first UTF-8 char.
            split_at = remaining
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
        }

        let chunk = &remaining[..split_at];
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }
        remaining = remaining[split_at..].trim_start_matches('\n');
    }

    chunks
}

/// Escape HTML special characters for Telegram HTML parse mode.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Convert standard Markdown (from LLM output) to Telegram HTML format.
///
/// Supported conversions:
/// - `## Title` → `<b>Title</b>`
/// - `**bold**` → `<b>bold</b>`
/// - `*italic*` → `<i>italic</i>`
/// - `` `code` `` → `<code>code</code>`
/// - ```` ```lang ... ``` ```` → `<pre><code>...</code></pre>`
/// - `[text](url)` → `<a href="url">text</a>`
/// - `~~text~~` → `<s>text</s>`
#[allow(clippy::collapsible_if)]
pub fn markdown_to_telegram_html(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut result_lines: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed_line = line.trim_start();

        // Preserve fence lines for the second-pass code block parser
        if trimmed_line.starts_with("```") {
            result_lines.push(trimmed_line.to_string());
            continue;
        }

        // Handle headers: ## Title → <b>Title</b>
        let stripped = line.trim_start_matches('#');
        let header_level = line.len() - stripped.len();
        if header_level > 0 && line.starts_with('#') && stripped.starts_with(' ') {
            let title = escape_html(stripped.trim());
            result_lines.push(format!("<b>{title}</b>"));
            continue;
        }

        // Inline formatting
        let mut line_out = String::new();
        let mut i = 0;
        let bytes = line.as_bytes();
        let len = bytes.len();

        while i < len {
            // Bold: **text**
            if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'*' {
                if let Some(end) = line[i + 2..].find("**") {
                    let inner = escape_html(&line[i + 2..i + 2 + end]);
                    line_out.push_str(&format!("<b>{inner}</b>"));
                    i += 4 + end;
                    continue;
                }
            }
            // Bold: __text__
            if i + 1 < len && bytes[i] == b'_' && bytes[i + 1] == b'_' {
                if let Some(end) = line[i + 2..].find("__") {
                    let inner = escape_html(&line[i + 2..i + 2 + end]);
                    line_out.push_str(&format!("<b>{inner}</b>"));
                    i += 4 + end;
                    continue;
                }
            }
            // Italic: *text* (single, not preceded by *)
            if bytes[i] == b'*' && (i == 0 || bytes[i - 1] != b'*') {
                if let Some(end) = line[i + 1..].find('*') {
                    if end > 0 {
                        let inner = escape_html(&line[i + 1..i + 1 + end]);
                        line_out.push_str(&format!("<i>{inner}</i>"));
                        i += 2 + end;
                        continue;
                    }
                }
            }
            // Inline code: `code`
            if bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'`') {
                if let Some(end) = line[i + 1..].find('`') {
                    let inner = escape_html(&line[i + 1..i + 1 + end]);
                    line_out.push_str(&format!("<code>{inner}</code>"));
                    i += 2 + end;
                    continue;
                }
            }
            // Link: [text](url)
            if bytes[i] == b'[' {
                if let Some(bracket_end) = line[i + 1..].find(']') {
                    let text_part = &line[i + 1..i + 1 + bracket_end];
                    let after_bracket = i + 1 + bracket_end + 1;
                    if after_bracket < len && bytes[after_bracket] == b'(' {
                        if let Some(paren_end) = line[after_bracket + 1..].find(')') {
                            let url = &line[after_bracket + 1..after_bracket + 1 + paren_end];
                            if url.starts_with("http://") || url.starts_with("https://") {
                                let text_html = escape_html(text_part);
                                let url_html = escape_html(url);
                                line_out
                                    .push_str(&format!("<a href=\"{url_html}\">{text_html}</a>"));
                                i = after_bracket + 1 + paren_end + 1;
                                continue;
                            }
                        }
                    }
                }
            }
            // Strikethrough: ~~text~~
            if i + 1 < len && bytes[i] == b'~' && bytes[i + 1] == b'~' {
                if let Some(end) = line[i + 2..].find("~~") {
                    let inner = escape_html(&line[i + 2..i + 2 + end]);
                    line_out.push_str(&format!("<s>{inner}</s>"));
                    i += 4 + end;
                    continue;
                }
            }
            // Default: escape HTML entities
            let ch = line[i..].chars().next().unwrap();
            match ch {
                '<' => line_out.push_str("&lt;"),
                '>' => line_out.push_str("&gt;"),
                '&' => line_out.push_str("&amp;"),
                '"' => line_out.push_str("&quot;"),
                '\'' => line_out.push_str("&#39;"),
                _ => line_out.push(ch),
            }
            i += ch.len_utf8();
        }
        result_lines.push(line_out);
    }

    // Second pass: merge ``` fenced code blocks into <pre><code> blocks
    let joined = result_lines.join("\n");
    let mut final_out = String::with_capacity(joined.len());
    let mut in_code_block = false;
    let mut code_buf = String::new();

    for line in joined.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if !in_code_block {
                in_code_block = true;
                code_buf.clear();
            } else {
                in_code_block = false;
                let escaped = code_buf.trim_end_matches('\n');
                final_out.push_str(&format!("<pre><code>{escaped}</code></pre>\n"));
                code_buf.clear();
            }
        } else if in_code_block {
            code_buf.push_str(line);
            code_buf.push('\n');
        } else {
            final_out.push_str(line);
            final_out.push('\n');
        }
    }
    // Handle unclosed code block
    if in_code_block && !code_buf.is_empty() {
        final_out.push_str(&format!(
            "<pre><code>{}</code></pre>\n",
            code_buf.trim_end()
        ));
    }

    final_out.trim_end_matches('\n').to_string()
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
    fn split_utf8_small_limit_no_empty_chunks() {
        let text = "你你你";
        let chunks = split_message(text, 1);
        assert_eq!(chunks, vec!["你", "你", "你"]);
    }

    #[test]
    fn split_zero_limit_returns_empty() {
        let chunks = split_message("hello", 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn split_utf8_preserves_valid_boundaries() {
        let text = "你好\n世界";
        let chunks = split_message(text, 2);
        assert_eq!(chunks, vec!["你", "好", "世", "界"]);
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

    #[test]
    fn acl_allows_when_both_lists_empty() {
        assert!(acl_allows(&[], &[], "100", Some("alice"), "200"));
    }

    #[test]
    fn acl_denies_when_chat_not_allowed() {
        let allow_from = vec!["100".to_string()];
        let allow_chats = vec!["300".to_string()];
        assert!(!acl_allows(
            &allow_from,
            &allow_chats,
            "100",
            Some("alice"),
            "200"
        ));
    }

    #[test]
    fn acl_allows_by_username() {
        let allow_from = vec!["alice".to_string()];
        assert!(acl_allows(&allow_from, &[], "100", Some("alice"), "200"));
    }

    #[test]
    fn acl_denies_when_user_and_chat_mismatch() {
        let allow_from = vec!["999".to_string()];
        let allow_chats = vec!["300".to_string()];
        assert!(!acl_allows(
            &allow_from,
            &allow_chats,
            "100",
            Some("alice"),
            "200"
        ));
    }

    // --- markdown_to_telegram_html tests ---

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(escape_html("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn html_plain_text_passthrough() {
        assert_eq!(markdown_to_telegram_html("Hello, world!"), "Hello, world!");
    }

    #[test]
    fn html_header_to_bold() {
        assert_eq!(markdown_to_telegram_html("## Title"), "<b>Title</b>");
        assert_eq!(markdown_to_telegram_html("### Sub"), "<b>Sub</b>");
    }

    #[test]
    fn html_bold() {
        assert_eq!(
            markdown_to_telegram_html("This is **bold** text"),
            "This is <b>bold</b> text"
        );
    }

    #[test]
    fn html_italic() {
        assert_eq!(
            markdown_to_telegram_html("This is *italic* text"),
            "This is <i>italic</i> text"
        );
    }

    #[test]
    fn html_inline_code() {
        assert_eq!(
            markdown_to_telegram_html("Use `cargo test` to run"),
            "Use <code>cargo test</code> to run"
        );
    }

    #[test]
    fn html_code_block() {
        let input = "Before\n```rust\nfn main() {}\n```\nAfter";
        let output = markdown_to_telegram_html(input);
        assert!(output.contains("<pre><code>"));
        assert!(output.contains("fn main() {}"));
        assert!(output.contains("</code></pre>"));
        assert!(output.contains("Before"));
        assert!(output.contains("After"));
    }

    #[test]
    fn html_link() {
        assert_eq!(
            markdown_to_telegram_html("Visit [Rust](https://rust-lang.org) now"),
            "Visit <a href=\"https://rust-lang.org\">Rust</a> now"
        );
    }

    #[test]
    fn html_strikethrough() {
        assert_eq!(
            markdown_to_telegram_html("This is ~~deleted~~ text"),
            "This is <s>deleted</s> text"
        );
    }

    #[test]
    fn html_mixed_formatting() {
        let input = "## Summary\n\nThis is **bold** and *italic* with `code`.";
        let output = markdown_to_telegram_html(input);
        assert!(output.contains("<b>Summary</b>"));
        assert!(output.contains("<b>bold</b>"));
        assert!(output.contains("<i>italic</i>"));
        assert!(output.contains("<code>code</code>"));
    }

    #[test]
    fn html_escapes_in_plain_text() {
        assert_eq!(
            markdown_to_telegram_html("a < b && c > d"),
            "a &lt; b &amp;&amp; c &gt; d"
        );
    }
}
