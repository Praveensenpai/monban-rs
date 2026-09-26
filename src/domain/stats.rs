use chrono::{FixedOffset, Utc};
use std::time::Instant;

const BAR_WIDTH: usize = 10;

/// In-memory alert statistics with per-hour histogram (resets on restart).
pub struct SentryStats {
    pub total_alerts: u64,
    pub session_count: u64,
    pub frames_processed: u64,
    hourly_histogram: [u32; 24],
    started_at: Instant,
}

impl SentryStats {
    pub fn new() -> Self {
        Self {
            total_alerts: 0,
            session_count: 0,
            frames_processed: 0,
            hourly_histogram: [0; 24],
            started_at: Instant::now(),
        }
    }

    pub fn record_alert(&mut self) {
        self.total_alerts += 1;
        self.hourly_histogram[ist_hour() as usize] += 1;
    }

    pub fn record_session(&mut self) {
        self.session_count += 1;
    }

    pub fn record_frame(&mut self) {
        self.frames_processed += 1;
    }

    pub fn format_message(&self) -> String {
        let uptime = self.started_at.elapsed().as_secs();
        let uptime_str = format!(
            "{}h {}m {}s",
            uptime / 3600,
            (uptime % 3600) / 60,
            uptime % 60
        );

        let max_val = self
            .hourly_histogram
            .iter()
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        let histogram = self
            .hourly_histogram
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| *v > 0)
            .map(|(h, v)| {
                let filled = (v as usize * BAR_WIDTH / max_val as usize).max(1);
                let bar = "█".repeat(filled) + &"░".repeat(BAR_WIDTH - filled);
                format!("  {h:02}h ┤{bar}┊ {v}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let histogram_str = if histogram.is_empty() {
            "  (no alerts yet)".to_string()
        } else {
            histogram
        };

        format!(
            "📊 <b>MONBAN STATS</b>\n\n\
            🚨 Alerts: <code>{}</code>\n\
            📹 Sessions: <code>{}</code>\n\
            🖼️ Frames: <code>{}</code>\n\
            ⏱️ Uptime: <code>{uptime_str}</code>\n\n\
            ⏰ <b>Alerts by hour (IST):</b>\n{histogram_str}",
            self.total_alerts, self.session_count, self.frames_processed
        )
    }
}

impl Default for SentryStats {
    fn default() -> Self {
        Self::new()
    }
}

fn ist_hour() -> u32 {
    FixedOffset::east_opt(19800)
        .map(|tz| {
            Utc::now()
                .with_timezone(&tz)
                .format("%H")
                .to_string()
                .parse()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}
