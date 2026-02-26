use std::sync::Arc;

use tracing::info;

use crate::channels::base::Channel;
use crate::channels::telegram::TelegramChannel;
use crate::core::config::AppConfig;
use crate::core::error::Result;

/// Manages channel lifecycles.
///
/// Aligned with bub's `ChannelManager`:
/// - Registers enabled channels based on config
/// - Runs all channels concurrently
pub struct ChannelManager {
    channels: Vec<Box<dyn Channel>>,
}

impl ChannelManager {
    pub fn new(config: Arc<AppConfig>, workspace: &std::path::Path) -> Self {
        let mut channels: Vec<Box<dyn Channel>> = Vec::new();

        if config.telegram_enabled() {
            info!("channel_manager.register: telegram");
            channels.push(Box::new(TelegramChannel::new(
                Arc::clone(&config),
                workspace.to_path_buf(),
            )));
        }

        Self { channels }
    }

    pub fn enabled_channels(&self) -> Vec<&str> {
        self.channels.iter().map(|c| c.name()).collect()
    }

    /// Run all registered channels. Blocks until all channels complete or error.
    pub async fn run(&mut self) -> Result<()> {
        if self.channels.is_empty() {
            return Err(crate::core::error::CrabClawError::Config(
                "no channels enabled; set TELEGRAM_TOKEN to enable Telegram".to_string(),
            ));
        }

        info!(
            "channel_manager.start channels={:?}",
            self.enabled_channels()
        );

        // For now, run the first channel (single-channel MVP).
        // Multi-channel concurrency will be added when Discord is implemented.
        if let Some(channel) = self.channels.first_mut() {
            channel.start().await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(telegram_token: Option<&str>) -> Arc<AppConfig> {
        Arc::new(AppConfig {
            profile: "test".to_string(),
            api_key: "key".to_string(),
            api_base: "https://api.example.com".to_string(),
            model: "test-model".to_string(),
            system_prompt: None,
            telegram_token: telegram_token.map(String::from),
            telegram_allow_from: vec![],
            telegram_allow_chats: vec![],
            telegram_proxy: None,
            max_context_messages: 50,
        })
    }

    #[test]
    fn no_channels_when_token_missing() {
        let config = test_config(None);
        let mgr = ChannelManager::new(config, std::path::Path::new("/tmp"));
        assert!(mgr.enabled_channels().is_empty());
    }

    #[test]
    fn telegram_registered_when_token_set() {
        let config = test_config(Some("test-token"));
        let mgr = ChannelManager::new(config, std::path::Path::new("/tmp"));
        assert_eq!(mgr.enabled_channels(), vec!["telegram"]);
    }
}
