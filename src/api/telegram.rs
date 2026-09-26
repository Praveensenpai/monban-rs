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
    Snap,
    Help,
}

#[derive(Deserialize)]
struct UpdateResponse {
    result: Option<Vec<UpdateItem>>,
}

#[derive(Deserialize)]
struct UpdateItem {
    update_id: i64,
    message: Option<MessageItem>,
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
            .text("parse_mode", "HTML");

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

                let Some(msg) = item.message else { continue };
                if msg.chat.id.to_string() != *chat_id {
                    continue;
                }

                let Some(text) = msg.text else { continue };
                let cmd_str = match text.split_whitespace().next() {
                    Some(s) => s.to_lowercase(),
                    None => continue,
                };

                let clean_cmd = cmd_str.split('@').next().unwrap_or(cmd_str.as_str());
                match clean_cmd {
                    "/status" => commands.push(BotCommand::Status),
                    "/arm" => commands.push(BotCommand::Arm),
                    "/disarm" => commands.push(BotCommand::Disarm),
                    "/snap" => commands.push(BotCommand::Snap),
                    "/help" | "/start" => commands.push(BotCommand::Help),
                    _ => {}
                }
            }
        }

        Ok(commands)
    }
}
