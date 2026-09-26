use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SentryConfig {
    pub source: String,
    pub model_path: PathBuf,
    pub confidence_threshold: f32,
    pub cooldown_seconds: u64,
    pub telegram_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    pub save_dir: PathBuf,
}

#[derive(Deserialize, Default)]
struct TayoriConfig {
    telegram: Option<TayoriTelegram>,
}

#[derive(Deserialize, Default)]
struct TayoriTelegram {
    bot_token: Option<String>,
    chat_id: Option<String>,
}

impl SentryConfig {
    pub fn load_with_defaults(
        source: Option<String>,
        model_path: Option<PathBuf>,
        confidence: Option<f32>,
        cooldown: Option<u64>,
        save_dir: Option<PathBuf>,
    ) -> Self {
        let (token, chat_id) = Self::discover_telegram_credentials();
        let default_model = if Path::new("yolov8n.onnx").exists() {
            PathBuf::from("yolov8n.onnx")
        } else {
            let home = std::env::var("HOME").unwrap_or_default();
            Path::new(&home).join(".local/share/monban/yolov8n.onnx")
        };

        Self {
            source: source.unwrap_or_else(|| "http://192.168.1.36:4747/video".to_string()),
            model_path: model_path.unwrap_or(default_model),
            confidence_threshold: confidence.unwrap_or(0.35),
            cooldown_seconds: cooldown.unwrap_or(5),
            telegram_token: token,
            telegram_chat_id: chat_id,
            save_dir: save_dir.unwrap_or_else(|| PathBuf::from("captures")),
        }
    }

    fn discover_telegram_credentials() -> (Option<String>, Option<String>) {
        if let (Ok(token), Ok(chat)) = (
            std::env::var("TELEGRAM_BOT_TOKEN"),
            std::env::var("TELEGRAM_CHAT_ID"),
        ) {
            return (Some(token), Some(chat));
        }

        Self::read_tayori_credentials().unwrap_or((None, None))
    }

    fn read_tayori_credentials() -> Option<(Option<String>, Option<String>)> {
        let home = std::env::var("HOME").ok()?;
        let tayori_path = Path::new(&home).join(".config/tayori/config.toml");
        let content = std::fs::read_to_string(tayori_path).ok()?;
        let parsed: TayoriConfig = toml::from_str(&content).ok()?;
        let tg = parsed.telegram?;
        Some((tg.bot_token, tg.chat_id))
    }
}
