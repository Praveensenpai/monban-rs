pub mod types;

pub use types::{BotCommand, OutgoingMessage};
use types::{CallbackQueryItem, MediaPayload, UpdateResponse, guardian_inline_markup};

use crate::error::{MonbanError, Result};
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct TelegramClient {
    token: Option<String>,
    chat_id: Option<String>,
    client: Client,
    last_update_id: Option<i64>,
    sender: Option<SyncSender<OutgoingMessage>>,
}

impl TelegramClient {
    pub fn new(token: Option<String>, chat_id: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        let sender = match (&token, &chat_id) {
            (Some(tok), Some(chat)) => {
                let (tx, rx) = sync_channel::<OutgoingMessage>(10);
                Self::spawn_dispatcher(tok.clone(), chat.clone(), client.clone(), rx);
                Some(tx)
            }
            _ => None,
        };

        Self {
            token,
            chat_id,
            client,
            last_update_id: None,
            sender,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some() && self.chat_id.is_some()
    }

    pub fn send_message(&self, text: &str) -> Result<bool> {
        let Some(sender) = &self.sender else {
            return Ok(false);
        };
        match sender.try_send(OutgoingMessage::Text(text.to_string())) {
            Ok(_) => Ok(true),
            Err(TrySendError::Full(_)) => {
                warn!("Telegram queue full. Dropping message.");
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => Ok(false),
        }
    }

    pub fn send_photo_alert(&self, image_bytes: Vec<u8>, caption: &str) -> Result<bool> {
        let Some(sender) = &self.sender else {
            warn!("Telegram not configured. Skipping alert dispatch.");
            return Ok(false);
        };
        match sender.try_send(OutgoingMessage::Photo {
            bytes: image_bytes,
            caption: caption.to_string(),
        }) {
            Ok(_) => Ok(true),
            Err(TrySendError::Full(_)) => {
                warn!("Telegram queue full. Dropping photo alert.");
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => Ok(false),
        }
    }

    pub fn send_animation_alert(&self, gif_bytes: Vec<u8>, caption: &str) -> Result<bool> {
        let Some(sender) = &self.sender else {
            warn!("Telegram not configured. Skipping animation dispatch.");
            return Ok(false);
        };
        match sender.try_send(OutgoingMessage::Animation {
            bytes: gif_bytes,
            caption: caption.to_string(),
        }) {
            Ok(_) => Ok(true),
            Err(TrySendError::Full(_)) => {
                warn!("Telegram queue full. Dropping animation alert.");
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => Ok(false),
        }
    }

    pub fn send_video_alert(&self, video_bytes: Vec<u8>, caption: &str) -> Result<bool> {
        let Some(sender) = &self.sender else {
            warn!("Telegram not configured. Skipping video dispatch.");
            return Ok(false);
        };
        match sender.try_send(OutgoingMessage::Video {
            bytes: video_bytes,
            caption: caption.to_string(),
        }) {
            Ok(_) => Ok(true),
            Err(TrySendError::Full(_)) => {
                warn!("Telegram queue full. Dropping video alert.");
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => Ok(false),
        }
    }

    fn spawn_dispatcher(
        token: String,
        chat_id: String,
        client: Client,
        rx: Receiver<OutgoingMessage>,
    ) {
        thread::spawn(move || {
            let mut last_send = Instant::now() - Duration::from_secs(2);
            while let Ok(msg) = rx.recv() {
                let elapsed = last_send.elapsed();
                if elapsed < Duration::from_millis(1000) {
                    thread::sleep(Duration::from_millis(1000) - elapsed);
                }

                let mut retries = 0;
                while retries < 3 {
                    match Self::dispatch_raw(&client, &token, &chat_id, &msg) {
                        Ok(true) => break,
                        Ok(false) => {
                            retries += 1;
                            thread::sleep(Duration::from_secs(2));
                        }
                        Err(e) => {
                            warn!("Telegram dispatch error: {e}");
                            retries += 1;
                            thread::sleep(Duration::from_secs(2));
                        }
                    }
                }
                last_send = Instant::now();
            }
        });
    }

    fn dispatch_raw(
        client: &Client,
        token: &str,
        chat_id: &str,
        msg: &OutgoingMessage,
    ) -> Result<bool> {
        match msg {
            OutgoingMessage::Text(text) => {
                let url = format!("https://api.telegram.org/bot{token}/sendMessage");
                let form = Form::new()
                    .text("chat_id", chat_id.to_string())
                    .text("text", text.clone())
                    .text("parse_mode", "HTML")
                    .text("reply_markup", guardian_inline_markup());

                let response = client.post(&url).multipart(form).send()?;
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    let delay = Self::parse_retry_after(&response);
                    thread::sleep(Duration::from_secs(delay));
                    return Ok(false);
                }
                Ok(response.status().is_success())
            }
            OutgoingMessage::Photo { bytes, caption } => Self::dispatch_media(
                client,
                token,
                chat_id,
                &MediaPayload {
                    endpoint: "sendPhoto",
                    field: "photo",
                    filename: "alert.jpg",
                    mime: "image/jpeg",
                    bytes,
                    caption,
                },
            ),
            OutgoingMessage::Animation { bytes, caption } => Self::dispatch_media(
                client,
                token,
                chat_id,
                &MediaPayload {
                    endpoint: "sendAnimation",
                    field: "animation",
                    filename: "motion.gif",
                    mime: "image/gif",
                    bytes,
                    caption,
                },
            ),
            OutgoingMessage::Video { bytes, caption } => Self::dispatch_media(
                client,
                token,
                chat_id,
                &MediaPayload {
                    endpoint: "sendVideo",
                    field: "video",
                    filename: "clip.mp4",
                    mime: "video/mp4",
                    bytes,
                    caption,
                },
            ),
        }
    }

    fn dispatch_media(
        client: &Client,
        token: &str,
        chat_id: &str,
        p: &MediaPayload<'_>,
    ) -> Result<bool> {
        let url = format!("https://api.telegram.org/bot{token}/{}", p.endpoint);
        let part = Part::bytes(p.bytes.to_vec())
            .file_name(p.filename)
            .mime_str(p.mime)
            .map_err(|e| MonbanError::Config(e.to_string()))?;

        let form = Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", p.caption.to_string())
            .text("parse_mode", "HTML")
            .text("reply_markup", guardian_inline_markup())
            .part(p.field, part);

        let response = client.post(&url).multipart(form).send()?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let delay = Self::parse_retry_after(&response);
            thread::sleep(Duration::from_secs(delay));
            return Ok(false);
        }
        if response.status().is_success() {
            info!("Telegram {} dispatched successfully.", p.endpoint);
            Ok(true)
        } else {
            error!(
                "Telegram {} error ({}): {:?}",
                p.endpoint,
                response.status(),
                response.text()
            );
            Ok(false)
        }
    }

    fn parse_retry_after(response: &reqwest::blocking::Response) -> u64 {
        response
            .headers()
            .get("Retry-After")
            .and_then(|val| val.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|secs| secs.clamp(1, 30))
            .unwrap_or(3)
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
            "stats" => {
                let _ = self.answer_callback(&cq.id, "📊 Gathering sentry statistics...");
                Some(BotCommand::Stats)
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
            "/stats" => Some(BotCommand::Stats),
            "/arm" => Some(BotCommand::Arm),
            "/disarm" => Some(BotCommand::Disarm),
            "/mute" => Some(BotCommand::Mute(10)),
            "/snap" => Some(BotCommand::Snap),
            "/help" | "/start" => Some(BotCommand::Help),
            _ => None,
        }
    }
}
