use crate::api::{BotCommand, TelegramClient};
use crate::domain::{Detection, SentryConfig};
use crate::error::{MonbanError, Result};
use crate::infra::{MjpegStream, MotionDetector, YoloDetector};
use chrono::{FixedOffset, Utc};
use image::{DynamicImage, ImageFormat};
use std::collections::HashMap;
use std::io::Cursor;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub struct RoomSentry {
    config: SentryConfig,
    detector: YoloDetector,
    motion_detector: MotionDetector,
    telegram: TelegramClient,
    last_alert: Option<Instant>,
    last_person_seen: Option<Instant>,
    last_command_poll: Instant,
    armed: bool,
    mute_until: Option<Instant>,
    start_time: Instant,
}

impl RoomSentry {
    pub fn new(config: SentryConfig) -> Result<Self> {
        let detector = YoloDetector::new(&config.model_path)?;
        let motion_detector = MotionDetector::new(config.motion_threshold);
        let telegram = TelegramClient::new(
            config.telegram_token.clone(),
            config.telegram_chat_id.clone(),
        );

        if !config.save_dir.exists() {
            std::fs::create_dir_all(&config.save_dir)?;
        }

        Ok(Self {
            config,
            detector,
            motion_detector,
            telegram,
            last_alert: None,
            last_person_seen: None,
            last_command_poll: Instant::now(),
            armed: true,
            mute_until: None,
            start_time: Instant::now(),
        })
    }

    pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>> {
        let has_motion = if self.config.motion_gate {
            self.motion_detector.check_motion(image)
        } else {
            true
        };

        let in_grace_period = self
            .last_person_seen
            .map(|t| t.elapsed() < Duration::from_secs(3))
            .unwrap_or(false);

        if !has_motion && !in_grace_period {
            debug!("Static frame: skipping YOLO inference");
            return Ok(Vec::new());
        }

        let detections = self.detector.detect(
            image,
            self.config.confidence_threshold,
            &self.config.targets,
        )?;

        if !detections.is_empty() {
            self.last_person_seen = Some(Instant::now());
            if self.is_alert_enabled() {
                self.handle_alert(image, &detections)?;
            } else {
                debug!("Sentry muted or disarmed: alert suppressed.");
            }
        }

        Ok(detections)
    }

    fn is_alert_enabled(&self) -> bool {
        if !self.armed {
            return false;
        }
        if let Some(until) = self.mute_until {
            Instant::now() >= until
        } else {
            true
        }
    }

    fn is_in_cooldown(&self) -> bool {
        if let Some(last) = self.last_alert {
            last.elapsed() < Duration::from_secs(self.config.cooldown_seconds)
        } else {
            false
        }
    }

    fn handle_alert(&mut self, image: &DynamicImage, detections: &[Detection]) -> Result<()> {
        if self.is_in_cooldown() {
            return Ok(());
        }

        let annotated = YoloDetector::annotate_frame(image, detections);
        let tz = FixedOffset::east_opt(19800)
            .ok_or_else(|| MonbanError::Config("Invalid IST offset".to_string()))?;
        let now = Utc::now().with_timezone(&tz);
        let filename = format!("sentry_{}.jpg", now.format("%Y%m%d_%H%M%S"));
        let save_path = self.config.save_dir.join(&filename);

        annotated.save_with_format(&save_path, ImageFormat::Jpeg)?;

        let mut jpeg_bytes = Vec::new();
        annotated.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)?;

        let summary_str = Self::format_detection_summary(detections);
        let caption = format!(
            "🚨 <b>MONBAN ALERT — Motion Detected</b>\n\n\
            {summary_str}\n\n\
            🕒 <b>Time:</b> {}\n\
            📁 <b>Evidence:</b> <code>{filename}</code>",
            now.format("%Y-%m-%d %H:%M:%S IST")
        );

        let _ = self.telegram.send_photo_alert(jpeg_bytes, &caption);
        self.last_alert = Some(Instant::now());
        warn!(
            "🚨 Alert triggered: {} target(s) detected!",
            detections.len()
        );
        Ok(())
    }

    fn format_detection_summary(detections: &[Detection]) -> String {
        let mut counts: HashMap<&str, (usize, f32)> = HashMap::new();
        for d in detections {
            let entry = counts.entry(&d.label).or_insert((0, 0.0));
            entry.0 += 1;
            if d.confidence > entry.1 {
                entry.1 = d.confidence;
            }
        }

        let mut summary_lines = Vec::new();
        for (label, (count, max_conf)) in counts {
            let emoji = crate::domain::class_emoji(label);
            summary_lines.push(format!(
                "{emoji} <b>{label}:</b> {count} ({:.1}%)",
                max_conf * 100.0
            ));
        }
        summary_lines.join("\n")
    }

    fn handle_commands(&mut self, current_frame: Option<&DynamicImage>) -> Result<()> {
        if self.last_command_poll.elapsed() < Duration::from_millis(1500) {
            return Ok(());
        }
        self.last_command_poll = Instant::now();

        let commands = self.telegram.poll_commands()?;
        for cmd in commands {
            self.execute_command(cmd, current_frame)?;
        }
        Ok(())
    }

    fn execute_command(
        &mut self,
        cmd: BotCommand,
        current_frame: Option<&DynamicImage>,
    ) -> Result<()> {
        match cmd {
            BotCommand::Status => self.send_status_reply()?,
            BotCommand::Arm => {
                self.armed = true;
                self.mute_until = None;
                self.telegram.send_message(
                    "⚔️ <b>SENTRY ARMED</b>\nIntruder detection alerts are now ACTIVE.",
                )?;
                info!("Sentry ARMED via Telegram command.");
            }
            BotCommand::Disarm => {
                self.armed = false;
                self.mute_until = None;
                self.telegram.send_message(
                    "🛑 <b>SENTRY DISARMED</b>\nIntruder alerts MUTED until re-armed.",
                )?;
                info!("Sentry DISARMED via Telegram command.");
            }
            BotCommand::Mute(mins) => self.mute_alerts(mins)?,
            BotCommand::Snap => match current_frame {
                Some(frame) => self.send_snapshot_reply(frame)?,
                None => {
                    self.telegram
                        .send_message("⚠️ Camera frame not yet available for snapshot.")?;
                }
            },
            BotCommand::Help => self.send_help_reply()?,
        }
        Ok(())
    }

    fn mute_alerts(&mut self, mins: u64) -> Result<()> {
        let duration = Duration::from_secs(mins * 60);
        self.mute_until = Some(Instant::now() + duration);
        let unmute_time_str = match FixedOffset::east_opt(19800) {
            Some(tz) => (Utc::now() + chrono::Duration::minutes(mins as i64))
                .with_timezone(&tz)
                .format("%H:%M:%S IST")
                .to_string(),
            None => (Utc::now() + chrono::Duration::minutes(mins as i64))
                .format("%H:%M:%S UTC")
                .to_string(),
        };
        self.telegram.send_message(&format!(
            "🛡️ <b>SENTRY MUTED FOR {}m</b>\nAlerts will automatically resume at {}.",
            mins, unmute_time_str
        ))?;
        info!("Sentry MUTED for {mins} minutes via Telegram command.");
        Ok(())
    }

    fn send_help_reply(&self) -> Result<()> {
        self.telegram.send_message(
            "🥋 <b>門番 (Monban) Guardian Commands</b>\n\n\
            • <code>/status</code> — Current sentry health & statistics\n\
            • <code>/snap</code> — Real-time camera snapshot\n\
            • <code>/mute</code> — Mute alerts for 10 minutes\n\
            • <code>/arm</code> — Enable intruder alerts\n\
            • <code>/disarm</code> — Mute intruder alerts\n\
            • <code>/help</code> — Show this commands menu",
        )?;
        Ok(())
    }

    fn send_status_reply(&self) -> Result<()> {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let uptime_str = format!(
            "{}h {}m {}s",
            uptime_secs / 3600,
            (uptime_secs % 3600) / 60,
            uptime_secs % 60
        );

        let arm_str = if !self.is_alert_enabled() {
            if let Some(until) = self.mute_until {
                let remaining = until.saturating_duration_since(Instant::now()).as_secs();
                format!("🛡️ <b>MUTED</b> ({}s remaining)", remaining)
            } else {
                "🛑 <b>DISARMED</b>".to_string()
            }
        } else {
            "⚔️ <b>ARMED</b>".to_string()
        };

        let mg_str = if self.config.motion_gate {
            "Active (128x96)"
        } else {
            "Disabled"
        };

        let last_alert_str = match self.last_alert {
            Some(t) => {
                let ago = t.elapsed().as_secs();
                format!("{ago}s ago")
            }
            None => "None".to_string(),
        };

        let msg = format!(
            "🥋 <b>MONBAN SENTRY STATUS</b>\n\n\
            State: {}\n\
            Motion Gate: <code>{}</code>\n\
            Cooldown: <code>{}s</code>\n\
            Uptime: <code>{}</code>\n\
            Last Alert: <code>{}</code>\n\
            Stream: <code>{}</code>",
            arm_str,
            mg_str,
            self.config.cooldown_seconds,
            uptime_str,
            last_alert_str,
            self.config.source
        );

        self.telegram.send_message(&msg)?;
        Ok(())
    }

    fn send_snapshot_reply(&mut self, frame: &DynamicImage) -> Result<()> {
        let detections = self
            .detector
            .detect(
                frame,
                self.config.confidence_threshold,
                &self.config.targets,
            )
            .unwrap_or_default();

        let annotated = YoloDetector::annotate_frame(frame, &detections);
        let mut jpeg_bytes = Vec::new();
        annotated.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)?;

        let now_str = match FixedOffset::east_opt(19800) {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format("%Y-%m-%d %H:%M:%S IST")
                .to_string(),
            None => Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        };

        let lines: Vec<String> = detections
            .iter()
            .map(|d| {
                format!(
                    "{} {}: {:.1}%",
                    crate::domain::class_emoji(&d.label),
                    d.label,
                    d.confidence * 100.0
                )
            })
            .collect();
        let detected_summary = if lines.is_empty() {
            "None (All Clear)".to_string()
        } else {
            lines.join(", ")
        };

        let caption = format!(
            "📸 <b>Manual Snapshot Requested</b>\n\n🕒 <b>Time:</b> {now_str}\n🎯 <b>Objects Visible:</b> {detected_summary}"
        );

        self.telegram.send_photo_alert(jpeg_bytes, &caption)?;
        info!("Manual snapshot dispatched to Telegram.");
        Ok(())
    }

    pub fn run_test(&mut self) -> Result<()> {
        info!("Running single-frame test mode on: {}", self.config.source);
        let stream = MjpegStream::connect(&self.config.source)?;

        let start = Instant::now();
        let frame = loop {
            if let Ok(f) = stream.read_latest_frame() {
                break f;
            }
            if start.elapsed() > Duration::from_secs(4) {
                return Err(MonbanError::Stream(
                    "Timed out waiting for camera frame".to_string(),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let test_path = self.config.save_dir.join("test_snapshot_rs.jpg");
        frame.save_with_format(&test_path, ImageFormat::Jpeg)?;
        info!("Saved raw test frame to {:?}", test_path);

        let detections = self.process_frame(&frame)?;
        info!(
            "Test frame processed. Found {} target(s).",
            detections.len()
        );
        Ok(())
    }

    pub fn run_loop(&mut self) -> Result<()> {
        info!("Starting Monban Sentry on: {}", self.config.source);
        let stream = MjpegStream::connect(&self.config.source)?;

        loop {
            match stream.read_latest_frame() {
                Ok(frame) => {
                    if let Err(e) = self.process_frame(&frame) {
                        warn!("Error processing frame: {e}");
                    }
                    if let Err(e) = self.handle_commands(Some(&frame)) {
                        warn!("Error handling bot commands: {e}");
                    }
                }
                Err(MonbanError::Stream(_)) => {
                    if let Err(e) = self.handle_commands(None) {
                        warn!("Error handling bot commands: {e}");
                    }
                }
                Err(e) => return Err(e),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
