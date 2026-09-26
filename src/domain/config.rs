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
    pub motion_gate: bool,
    pub motion_threshold: f32,
}

#[derive(Deserialize, Default, Clone)]
pub struct MonbanConfigFile {
    pub telegram: Option<TelegramConfigSection>,
    pub sentry: Option<SentryConfigSection>,
}

#[derive(Deserialize, Default, Clone)]
pub struct TelegramConfigSection {
    pub bot_token: Option<String>,
    pub chat_id: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct SentryConfigSection {
    pub source: Option<String>,
    pub confidence: Option<f32>,
    pub cooldown: Option<u64>,
    pub motion_gate: Option<bool>,
    pub motion_threshold: Option<f32>,
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
    pub fn config_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(Path::new(&home).join(".config/monban"))
    }

    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|dir| dir.join("config.toml"))
    }

    pub fn load_with_defaults(
        source: Option<String>,
        model_path: Option<PathBuf>,
        confidence: Option<f32>,
        cooldown: Option<u64>,
        save_dir: Option<PathBuf>,
        motion_gate: Option<bool>,
        motion_threshold: Option<f32>,
    ) -> Self {
        let file_cfg = Self::read_monban_config().unwrap_or_default();
        let (token, chat_id) = Self::discover_telegram_credentials(&file_cfg);
        let sentry_sec = file_cfg.sentry.unwrap_or_default();

        let default_model = if Path::new("yolov8n.onnx").exists() {
            PathBuf::from("yolov8n.onnx")
        } else {
            let home = std::env::var("HOME").unwrap_or_default();
            Path::new(&home).join(".local/share/monban/yolov8n.onnx")
        };

        let resolved_model = match model_path {
            Some(p) if p.exists() => p,
            Some(p) => {
                if default_model.exists() {
                    default_model
                } else {
                    p
                }
            }
            None => default_model,
        };

        let src = source
            .or(sentry_sec.source)
            .unwrap_or_else(|| "http://192.168.1.36:4747/video".to_string());
        let conf = confidence.or(sentry_sec.confidence).unwrap_or(0.25);
        let cd = cooldown.or(sentry_sec.cooldown).unwrap_or(5);
        let mg = motion_gate.or(sentry_sec.motion_gate).unwrap_or(true);
        let mt = motion_threshold
            .or(sentry_sec.motion_threshold)
            .unwrap_or(0.005);

        Self {
            source: src,
            model_path: resolved_model,
            confidence_threshold: conf,
            cooldown_seconds: cd,
            telegram_token: token,
            telegram_chat_id: chat_id,
            save_dir: save_dir.unwrap_or_else(|| PathBuf::from("captures")),
            motion_gate: mg,
            motion_threshold: mt,
        }
    }

    fn discover_telegram_credentials(
        file_cfg: &MonbanConfigFile,
    ) -> (Option<String>, Option<String>) {
        if let (Ok(token), Ok(chat)) = (
            std::env::var("MONBAN_TELEGRAM_TOKEN"),
            std::env::var("MONBAN_TELEGRAM_CHAT_ID"),
        ) {
            return (Some(token), Some(chat));
        }

        if let (Ok(token), Ok(chat)) = (
            std::env::var("TELEGRAM_BOT_TOKEN"),
            std::env::var("TELEGRAM_CHAT_ID"),
        ) {
            return (Some(token), Some(chat));
        }

        if let Some(tg) = &file_cfg.telegram
            && tg.bot_token.is_some()
            && tg.chat_id.is_some()
        {
            return (tg.bot_token.clone(), tg.chat_id.clone());
        }

        Self::read_tayori_credentials().unwrap_or((None, None))
    }

    fn read_monban_config() -> Option<MonbanConfigFile> {
        let path = Self::config_path()?;
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
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
