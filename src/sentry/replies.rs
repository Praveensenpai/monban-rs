use crate::api::TelegramClient;
use crate::domain::{Detection, SentryConfig};
use crate::error::Result;
use crate::infra::YoloDetector;
use chrono::{FixedOffset, Utc};
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::time::{Duration, Instant};
use tracing::info;

pub fn mute_alerts(
    telegram: &TelegramClient,
    mute_until: &mut Option<Instant>,
    mins: u64,
) -> Result<()> {
    *mute_until = Some(Instant::now() + Duration::from_secs(mins * 60));
    let unmute_time_str = match FixedOffset::east_opt(19800) {
        Some(tz) => (Utc::now() + chrono::Duration::minutes(mins as i64))
            .with_timezone(&tz)
            .format("%H:%M:%S IST")
            .to_string(),
        None => (Utc::now() + chrono::Duration::minutes(mins as i64))
            .format("%H:%M:%S UTC")
            .to_string(),
    };
    telegram.send_message(&format!(
        "🛡️ <b>SENTRY MUTED FOR {mins}m</b>\nAlerts will automatically resume at {unmute_time_str}.",
    ))?;
    info!("Sentry MUTED for {mins} minutes via Telegram command.");
    Ok(())
}

pub fn send_help(telegram: &TelegramClient) -> Result<()> {
    telegram.send_message(
        "🥋 <b>門番 (Monban) Guardian Commands</b>\n\n\
        • <code>/status</code> — Current sentry health\n\
        • <code>/stats</code> — In-memory alert stats & hourly histogram\n\
        • <code>/snap</code> — Real-time camera snapshot\n\
        • <code>/mute</code> — Mute alerts for 10 minutes\n\
        • <code>/arm</code> — Enable intruder alerts\n\
        • <code>/disarm</code> — Mute intruder alerts\n\
        • <code>/help</code> — Show this commands menu",
    )?;
    Ok(())
}

pub fn send_stats(telegram: &TelegramClient, stats: &crate::domain::SentryStats) -> Result<()> {
    telegram.send_message(&stats.format_message())?;
    Ok(())
}

pub fn send_status(
    telegram: &TelegramClient,
    config: &SentryConfig,
    armed: bool,
    mute_until: Option<Instant>,
    last_alert: Option<Instant>,
    start_time: Instant,
) -> Result<()> {
    let uptime = start_time.elapsed().as_secs();
    let uptime_str = format!(
        "{}h {}m {}s",
        uptime / 3600,
        (uptime % 3600) / 60,
        uptime % 60
    );

    let arm_str = build_arm_str(armed, mute_until);
    let mg_str = if config.motion_gate {
        "Active (128x96)"
    } else {
        "Disabled"
    };
    let last_alert_str = last_alert
        .map(|t| format!("{}s ago", t.elapsed().as_secs()))
        .unwrap_or_else(|| "None".to_string());

    let itp_str = if config.ignore_top_percent > 0 {
        format!("{}% ignored", config.ignore_top_percent)
    } else {
        "Off".to_string()
    };

    let msg = format!(
        "🥋 <b>MONBAN SENTRY STATUS</b>\n\n\
        State: {arm_str}\n\
        Motion Gate: <code>{mg_str}</code>\n\
        ROI Exclusion: <code>{itp_str}</code>\n\
        Cooldown: <code>{}s</code>\n\
        Uptime: <code>{uptime_str}</code>\n\
        Last Alert: <code>{last_alert_str}</code>\n\
        Stream: <code>{}</code>",
        config.cooldown_seconds, config.source
    );

    telegram.send_message(&msg)?;
    Ok(())
}

pub fn send_snapshot(
    telegram: &TelegramClient,
    detector: &mut YoloDetector,
    config: &SentryConfig,
    frame: &DynamicImage,
) -> Result<()> {
    let detections = detector
        .detect(frame, config.confidence_threshold, &config.targets)
        .unwrap_or_default();

    let annotated = YoloDetector::annotate_frame(frame, &detections);
    let mut jpeg_bytes = Vec::new();
    annotated.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)?;

    let now_str = ist_now_str();
    let summary = snap_summary(&detections);

    let caption = format!(
        "📸 <b>Manual Snapshot Requested</b>\n\n🕒 <b>Time:</b> {now_str}\n🎯 <b>Status:</b> {summary}"
    );

    telegram.send_photo_alert(jpeg_bytes, &caption)?;
    info!("Manual snapshot dispatched to Telegram.");
    Ok(())
}

// ── helpers ─────────────────────────────────────────────────────────────────

pub fn ist_now_str() -> String {
    match FixedOffset::east_opt(19800) {
        Some(tz) => Utc::now()
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M:%S IST")
            .to_string(),
        None => Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    }
}

fn snap_summary(detections: &[Detection]) -> String {
    if detections.is_empty() {
        return "None (All Clear)".to_string();
    }
    let max_conf = detections
        .iter()
        .map(|d| d.confidence)
        .fold(0.0f32, f32::max);
    format!("Target Detected ({:.1}%)", max_conf * 100.0)
}

fn build_arm_str(armed: bool, mute_until: Option<Instant>) -> String {
    let is_alert_enabled = armed && mute_until.is_none_or(|until| Instant::now() >= until);
    if !is_alert_enabled {
        match mute_until {
            Some(until) => format!(
                "🛡️ <b>MUTED</b> ({}s remaining)",
                until.saturating_duration_since(Instant::now()).as_secs()
            ),
            None => "🛑 <b>DISARMED</b>".to_string(),
        }
    } else {
        "⚔️ <b>ARMED</b>".to_string()
    }
}
