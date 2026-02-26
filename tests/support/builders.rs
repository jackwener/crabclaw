use crabclaw::core::config::AppConfig;

pub fn openai_config(api_base: &str) -> AppConfig {
    AppConfig {
        profile: "test".to_string(),
        api_key: "test-key".to_string(),
        api_base: api_base.to_string(),
        model: "openai:test-model".to_string(),
        system_prompt: None,
        telegram_token: Some("fake-token".to_string()),
        telegram_allow_from: vec![],
        telegram_allow_chats: vec![],
        telegram_proxy: None,
        max_context_messages: 50,
    }
}

pub fn anthropic_config(api_base: &str) -> AppConfig {
    let mut cfg = openai_config(api_base);
    cfg.model = "anthropic:test-model".to_string();
    cfg
}
