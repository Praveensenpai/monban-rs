use std::time::{Duration, Instant};

/// Gap with no detection that closes an active alert session.
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(120);

/// Tracks a continuous presence event: start time, last seen, alert count, peak confidence.
pub struct AlertSession {
    pub started_at: Instant,
    pub last_seen: Instant,
    pub alert_count: u32,
    pub max_confidence: f32,
}

impl AlertSession {
    pub fn new(confidence: f32) -> Self {
        let now = Instant::now();
        Self {
            started_at: now,
            last_seen: now,
            alert_count: 1,
            max_confidence: confidence,
        }
    }

    /// Extend the session with a new detection observation.
    pub fn extend(&mut self, confidence: f32) {
        self.last_seen = Instant::now();
        self.alert_count += 1;
        self.max_confidence = self.max_confidence.max(confidence);
    }

    /// Returns true when no detection has been seen within SESSION_TIMEOUT.
    pub fn is_expired(&self) -> bool {
        self.last_seen.elapsed() > SESSION_TIMEOUT
    }

    /// Human-readable session duration.
    pub fn duration_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Telegram summary message dispatched when the session closes.
    pub fn summary_message(&self) -> String {
        format!(
            "🏁 <b>Session ended</b> — present for <code>{}s</code>, \
            peak <code>{:.1}%</code>, <code>{}</code> alert(s) fired.",
            self.duration_secs(),
            self.max_confidence * 100.0,
            self.alert_count
        )
    }
}

/// Extract peak detection confidence from a slice.
pub fn peak_confidence(detections: &[crate::domain::Detection]) -> f32 {
    detections
        .iter()
        .map(|d| d.confidence)
        .fold(0.0f32, f32::max)
}
