use crate::error::{MonbanError, Result};
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use std::time::Duration;
use tracing::{error, info, warn};

pub struct TelegramClient {
    token: Option<String>,
    chat_id: Option<String>,
    client: Client,
}

impl TelegramClient {
    pub fn new(token: Option<String>, chat_id: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            token,
            chat_id,
            client,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some() && self.chat_id.is_some()
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
}
