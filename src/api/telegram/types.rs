use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotCommand {
    Status,
    Stats,
    Arm,
    Disarm,
    Mute(u64),
    Snap,
    Help,
}

#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    Text(String),
    Photo { bytes: Vec<u8>, caption: String },
    Animation { bytes: Vec<u8>, caption: String },
    Video { bytes: Vec<u8>, caption: String },
}

pub struct MediaPayload<'a> {
    pub endpoint: &'static str,
    pub field: &'static str,
    pub filename: &'static str,
    pub mime: &'static str,
    pub bytes: &'a [u8],
    pub caption: &'a str,
}

pub fn guardian_inline_markup() -> &'static str {
    r#"{"inline_keyboard":[[{"text":"📸 Snapshot","callback_data":"snap"},{"text":"📊 Stats","callback_data":"stats"}],[{"text":"🛡️ Mute 10m","callback_data":"mute_10"},{"text":"⚔️ Arm","callback_data":"arm"},{"text":"🛑 Disarm","callback_data":"disarm"}]]}"#
}

#[derive(Deserialize)]
pub struct UpdateResponse {
    pub result: Option<Vec<UpdateItem>>,
}

#[derive(Deserialize)]
pub struct UpdateItem {
    pub update_id: i64,
    pub message: Option<MessageItem>,
    pub callback_query: Option<CallbackQueryItem>,
}

#[derive(Deserialize)]
pub struct MessageItem {
    pub chat: ChatItem,
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub struct ChatItem {
    pub id: i64,
}

#[derive(Deserialize)]
pub struct CallbackQueryItem {
    pub id: String,
    pub from: UserItem,
    pub data: Option<String>,
}

#[derive(Deserialize)]
pub struct UserItem {
    pub id: i64,
}
