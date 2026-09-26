use crate::api::TelegramClient;
use crate::domain::SentryConfig;
use crate::error::{MonbanError, Result};
use chrono::{FixedOffset, Utc};
use serde::Deserialize;
use std::io::{self, Write};

#[derive(Deserialize)]
struct UpdateResponse {
    result: Option<Vec<UpdateItem>>,
}

#[derive(Deserialize)]
struct UpdateItem {
    message: Option<MessageItem>,
}

#[derive(Deserialize)]
struct MessageItem {
    chat: ChatItem,
}

#[derive(Deserialize)]
struct ChatItem {
    id: i64,
}

pub fn run_interactive_setup() -> Result<()> {
    println!("\n🥋 門番 (Monban) — Interactive Bot & Sentry Setup");
    println!("═════════════════════════════════════════════════\n");
    println!("To set up a dedicated Telegram bot for Monban:");
    println!("1. Open Telegram and search for @BotFather");
    println!("2. Send /newbot and choose a name (e.g. 'MyMonbanBot')");
    println!("3. Copy the Bot API Token provided by BotFather\n");

    print!("👉 Enter Telegram Bot Token: ");
    io::stdout().flush().map_err(MonbanError::Io)?;
    let mut token = String::new();
    io::stdin().read_line(&mut token).map_err(MonbanError::Io)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(MonbanError::Config("Bot token cannot be empty".to_string()));
    }

    print!("👉 Enter Telegram Chat ID (leave blank to auto-detect): ");
    io::stdout().flush().map_err(MonbanError::Io)?;
    let mut chat_id_input = String::new();
    io::stdin()
        .read_line(&mut chat_id_input)
        .map_err(MonbanError::Io)?;
    let mut chat_id = chat_id_input.trim().to_string();

    if chat_id.is_empty() {
        println!("\n📲 Please open your new bot on Telegram, press START (or send /start),");
        print!("   then press [Enter] here to auto-detect your Chat ID... ");
        io::stdout().flush().map_err(MonbanError::Io)?;
        let mut _wait = String::new();
        let _ = io::stdin().read_line(&mut _wait);

        chat_id = auto_detect_chat_id(&token)?;
        println!("✅ Auto-detected Chat ID: {chat_id}");
    }

    println!("\n📡 Verifying connection with Telegram...");
    let client = TelegramClient::new(Some(token.clone()), Some(chat_id.clone()));

    let ist_offset = FixedOffset::east_opt(19800);
    let now = match ist_offset {
        Some(tz) => Utc::now()
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M:%S IST"),
        None => Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
    };

    let verify_message = format!(
        "🥋 <b>門番 (Monban) — Bot Verified!</b>\n\n\
        ✅ Your dedicated AI Room Sentry bot is officially linked and verified.\n\
        🕒 <b>Verified At:</b> {}\n\n\
        You can now control Monban using:\n\
        • <code>/status</code> — Sentry health\n\
        • <code>/snap</code> — Live snapshot\n\
        • <code>/arm</code> — Enable alerts\n\
        • <code>/disarm</code> — Mute alerts",
        now
    );

    let ok = client.send_message(&verify_message)?;
    if !ok {
        return Err(MonbanError::Config(
            "Failed to deliver verification message. Check your bot token and chat ID.".to_string(),
        ));
    }

    println!("✅ Test message successfully delivered to your Telegram!");

    save_monban_config(&token, &chat_id)?;
    println!("\n🎉 Setup complete! You can now run 'monban' or 'monban --help'.\n");

    Ok(())
}

fn auto_detect_chat_id(token: &str) -> Result<String> {
    let url = format!("https://api.telegram.org/bot{token}/getUpdates?timeout=0");
    let res = reqwest::blocking::get(&url)
        .map_err(MonbanError::Network)?
        .text()
        .map_err(MonbanError::Network)?;

    let parsed: UpdateResponse = serde_json::from_str(&res)
        .map_err(|e| MonbanError::Config(format!("JSON parse error: {e}")))?;

    let detected = parsed.result.as_ref().and_then(|updates| {
        updates
            .iter()
            .rev()
            .find_map(|item| item.message.as_ref().map(|m| m.chat.id.to_string()))
    });

    if let Some(chat_id) = detected {
        return Ok(chat_id);
    }

    Err(MonbanError::Config(
        "Could not detect Chat ID. Please send a message to your bot on Telegram and retry."
            .to_string(),
    ))
}

fn save_monban_config(token: &str, chat_id: &str) -> Result<()> {
    let config_dir = SentryConfig::config_dir()
        .ok_or_else(|| MonbanError::Config("Could not resolve HOME directory".to_string()))?;

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(MonbanError::Io)?;
    }

    let config_file = config_dir.join("config.toml");
    let content = format!(
        "# 🥋 門番 (Monban) AI Room & Door Sentry Configuration\n\n\
        [telegram]\n\
        bot_token = \"{token}\"\n\
        chat_id = \"{chat_id}\"\n\n\
        [sentry]\n\
        source = \"http://192.168.1.36:4747/video\"\n\
        confidence = 0.25\n\
        cooldown = 5\n\
        motion_gate = true\n\
        motion_threshold = 0.005\n"
    );

    std::fs::write(&config_file, content).map_err(MonbanError::Io)?;
    println!("💾 Configuration written to: {}", config_file.display());
    Ok(())
}
