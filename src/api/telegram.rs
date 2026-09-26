use crate::error::{MonbanError, Result};
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use serde::Deserialize;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotCommand {
    Status,
    Arm,
    Disarm,
    Mute(u64),
    Snap,
    Help,
}

fn guardian_inline_markup() -> &'static str {
    r#"{"inline_keyboard":[[{"text":"📸 Snapshot","callback_data":"snap"},{"text":"🛡️ Mute 10m","callback_data":"mute_10"}],[{"text":"⚔️ Arm","callback_data":"arm"},{"text":"🛑 Disarm","callback_data":"disarm"}]]}"#
}

#[derive(Deserialize)]
struct UpdateResponse {
    result: Option<Vec<UpdateItem>>,
}

#[derive(Deserialize)]
struct UpdateItem {
    update_id: i64,
    message: Option<MessageItem>,
    callback_query: Option<CallbackQueryItem>,
}

#[derive(Deserialize)]
struct MessageItem {
    chat: ChatItem,
    text: Option<String>,
}

#[derive(Deserialize)]
struct ChatItem {
    id: i64,
}

#[derive(Deserialize)]
struct CallbackQueryItem {
    id: String,
    from: UserItem,
    data: Option<String>,
}

#[derive(Deserialize)]
struct UserItem {
    id: i64,
}

pub struct TelegramClient {
    token: Option<String>,
    chat_id: Option<String>,
    client: Client,
    last_update_id: Option<i64>,
}

impl TelegramClient {
    pub fn new(token: Option<String>, chat_id: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            token,
            chat_id,
            client,
            last_update_id: None,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some() && self.chat_id.is_some()
    }

    pub fn send_message(&self, text: &str) -> Result<bool> {
        let (Some(token), Some(chat_id)) = (&self.token, &self.chat_id) else {
            return Ok(false);
        };

        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let form = Form::new()
            .text("chat_id", chat_id.clone())
            .text("text", text.to_string())
            .text("parse_mode", "HTML")
            .text("reply_markup", guardian_inline_markup());

        let response = self.client.post(&url).multipart(form).send()?;
        Ok(response.status().is_success())
    }

    pub fn send_photo_alert(&self, image_bytes: Vec<u8>, caption: &str) -> Result<bool> {
        let (Some(token), Some(chat_id)) = (&self.token, &self.chat_id) else {
            warn!("Telegram not configured. Skipping alert dispatch.");
            return Ok(false);
        };

        let url = format!("https://api.telegram.org/bot{token}/sendPhoto");
        let part = Part::bytes(image_bytes)
            .file_name("alert.jpg")
            .mime_str("image/jpeg")
            .map_err(|e| MonbanError::Config(e.to_string()))?;

        let form = Form::new()
            .text("chat_id", chat_id.clone())
            .text("caption", caption.to_string())
            .text("parse_mode", "HTML")
            .text("reply_markup", guardian_inline_markup())
            .part("photo", part);

        let response = self.client.post(&url).multipart(form).send()?;
        if response.status().is_success() {
            info!("Telegram alert dispatched successfully.");
            Ok(true)
        } else {
            error!(
                "Telegram error ({}): {:?}",
                response.status(),
                response.text()
            );
            Ok(false)
        }
    }

    pub fn answer_callback(&self, query_id: &str, toast: &str) -> Result<()> {
        let Some(token) = &self.token else {
            return Ok(());
        };
        let url = format!("https://api.telegram.org/bot{token}/answerCallbackQuery");
        let form = Form::new()
            .text("callback_query_id", query_id.to_string())
            .text("text", toast.to_string());
        let _ = self.client.post(&url).multipart(form).send();
        Ok(())
    }

    pub fn poll_commands(&mut self) -> Result<Vec<BotCommand>> {
        let (Some(token), Some(chat_id)) = (&self.token, &self.chat_id) else {
            return Ok(Vec::new());
        };

        let mut url = format!("https://api.telegram.org/bot{token}/getUpdates?timeout=0");
        if let Some(offset) = self.last_update_id {
            url.push_str(&format!("&offset={offset}"));
        }

        let response = match self.client.get(&url).send() {
            Ok(res) => res,
            Err(e) => {
                warn!("Failed to fetch Telegram updates: {e}");
                return Ok(Vec::new());
            }
        };

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let body = response.text()?;
        let data: UpdateResponse = match serde_json::from_str(&body) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to parse Telegram updates: {e}");
                return Ok(Vec::new());
            }
        };

        let mut commands = Vec::new();
        if let Some(updates) = data.result {
            for item in updates {
                self.last_update_id = Some(item.update_id + 1);

                if let Some(cq) = item.callback_query {
                    if cq.from.id.to_string() == *chat_id {
                        commands.extend(self.handle_callback_query(&cq));
                    }
                    continue;
                }

                if let Some(msg) = item.message
                    && msg.chat.id.to_string() == *chat_id
                {
                    commands.extend(Self::parse_text_command(msg.text.as_deref()));
                }
            }
        }

        Ok(commands)
    }

    fn handle_callback_query(&self, cq: &CallbackQueryItem) -> Option<BotCommand> {
        let data = cq.data.as_deref()?;
        match data {
            "snap" => {
                let _ = self.answer_callback(&cq.id, "📸 Capturing snapshot...");
                Some(BotCommand::Snap)
            }
            "mute_10" => {
                let _ = self.answer_callback(&cq.id, "🛡️ Muted for 10 minutes");
                Some(BotCommand::Mute(10))
            }
            "arm" => {
                let _ = self.answer_callback(&cq.id, "⚔️ Sentry Armed");
                Some(BotCommand::Arm)
            }
            "disarm" => {
                let _ = self.answer_callback(&cq.id, "🛑 Sentry Disarmed");
                Some(BotCommand::Disarm)
            }
            _ => None,
        }
    }

    fn parse_text_command(text: Option<&str>) -> Option<BotCommand> {
        let text = text?;
        let cmd_str = text.split_whitespace().next()?.to_lowercase();
        let clean_cmd = cmd_str.split('@').next().unwrap_or(cmd_str.as_str());

        match clean_cmd {
            "/status" => Some(BotCommand::Status),
            "/arm" => Some(BotCommand::Arm),
            "/disarm" => Some(BotCommand::Disarm),
            "/mute" => Some(BotCommand::Mute(10)),
            "/snap" => Some(BotCommand::Snap),
            "/help" | "/start" => Some(BotCommand::Help),
            _ => None,
        }
    }
}
