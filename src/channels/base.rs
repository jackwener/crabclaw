use async_trait::async_trait;
use serde::Serialize;

/// Metadata for a message received from a channel.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelMessage {
    /// Unique session identifier, e.g. "telegram:12345".
    pub session_id: String,
    /// The user-facing text content of the message.
    pub content: String,
    /// Channel-specific metadata (message_id, sender info, etc.).
    pub metadata: serde_json::Value,
}

/// Result of processing a channel message through the router/model.
#[derive(Debug, Clone, Default)]
pub struct ChannelResponse {
    /// Immediate output from command execution.
    pub immediate_output: Option<String>,
    /// Model-generated response.
    pub assistant_output: Option<String>,
    /// Error message, if any.
    pub error: Option<String>,
}

impl ChannelResponse {
    /// Combine all parts into a single reply string.
    pub fn to_reply(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(ref s) = self.immediate_output {
            parts.push(s.clone());
        }
        if let Some(ref s) = self.assistant_output {
            parts.push(s.clone());
        }
        if let Some(ref e) = self.error {
            parts.push(format!("Error: {e}"));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

/// Abstract channel adapter.
///
/// Aligned with bub's `BaseChannel`:
/// - `start()` begins receiving messages and calls the handler
/// - `stop()` performs graceful shutdown
#[async_trait]
pub trait Channel: Send + Sync {
    /// Channel name, e.g. "telegram", "discord".
    fn name(&self) -> &str;

    /// Start the channel and begin processing messages.
    async fn start(&mut self) -> crate::core::error::Result<()>;

    /// Stop the channel gracefully.
    async fn stop(&mut self) -> crate::core::error::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_response_to_reply_empty() {
        let r = ChannelResponse::default();
        assert!(r.to_reply().is_none());
    }

    #[test]
    fn channel_response_to_reply_with_immediate() {
        let r = ChannelResponse {
            immediate_output: Some("ok".to_string()),
            ..Default::default()
        };
        assert_eq!(r.to_reply().unwrap(), "ok");
    }

    #[test]
    fn channel_response_to_reply_combined() {
        let r = ChannelResponse {
            immediate_output: Some("cmd output".to_string()),
            assistant_output: Some("model reply".to_string()),
            error: None,
        };
        assert_eq!(r.to_reply().unwrap(), "cmd output\n\nmodel reply");
    }

    #[test]
    fn channel_message_serializes() {
        let msg = ChannelMessage {
            session_id: "telegram:123".to_string(),
            content: "hello".to_string(),
            metadata: serde_json::json!({"message_id": 42}),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["session_id"], "telegram:123");
        assert_eq!(json["content"], "hello");
        assert_eq!(json["metadata"]["message_id"], 42);
    }
}
