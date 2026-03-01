use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::{Mutex, mpsc};
use tracing::warn;

type NotifyKey = (String, i64);
type TelegramNotifySender = mpsc::UnboundedSender<String>;

static TELEGRAM_NOTIFY_SENDERS: OnceLock<Mutex<HashMap<NotifyKey, TelegramNotifySender>>> =
    OnceLock::new();

fn notifier_senders() -> &'static Mutex<HashMap<NotifyKey, TelegramNotifySender>> {
    TELEGRAM_NOTIFY_SENDERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or create a per-(token, chat_id) notifier sender backed by an async worker.
///
/// The worker serializes message delivery per key and reuses a single HTTP client.
pub async fn get_or_create_notifier_sender(token: &str, chat_id: i64) -> TelegramNotifySender {
    let key = (token.to_string(), chat_id);

    {
        let senders = notifier_senders().lock().await;
        if let Some(existing) = senders.get(&key).cloned() {
            return existing;
        }
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    {
        let mut senders = notifier_senders().lock().await;
        if let Some(existing) = senders.get(&key).cloned() {
            return existing;
        }
        senders.insert(key.clone(), tx.clone());
    }

    let token_owned = token.to_string();
    tokio::spawn(async move {
        let url = format!("https://api.telegram.org/bot{token_owned}/sendMessage");
        let client = reqwest::Client::new();
        while let Some(msg_text) = rx.recv().await {
            match client
                .post(&url)
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": msg_text,
                }))
                .send()
                .await
            {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        warn!(
                            chat_id = chat_id,
                            status = %resp.status(),
                            "telegram.notifier.send_message_failed"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        chat_id = chat_id,
                        error = %e,
                        "telegram.notifier.send_message_error"
                    );
                }
            }
        }

        let mut senders = notifier_senders().lock().await;
        senders.remove(&key);
    });

    tx
}
