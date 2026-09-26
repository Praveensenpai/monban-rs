pub mod dispatch;
pub mod replies;
pub mod runner;
pub mod session;

use crate::api::{BotCommand, TelegramClient};
use crate::domain::{Detection, SentryConfig, SentryStats};
use crate::error::Result;
use crate::infra::{MotionDetector, YoloDetector, fire_webhook, prune_old_evidence};
use image::{DynamicImage, ImageFormat};
use session::AlertSession;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

const FRAME_BUFFER_CAP: usize = 30;
const CONFIRM_WINDOW: Duration = Duration::from_millis(700);
const PRUNE_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub struct RoomSentry {
    pub(crate) config: SentryConfig,
    pub(crate) detector: YoloDetector,
    pub(crate) motion_detector: MotionDetector,
    pub(crate) telegram: TelegramClient,
    pub(crate) last_alert: Option<Instant>,
    pub(crate) last_person_seen: Option<Instant>,
    pub(crate) last_command_poll: Instant,
    pub(crate) last_prune: Instant,
    pub(crate) last_heartbeat: Instant,
    pub(crate) armed: bool,
    pub(crate) mute_until: Option<Instant>,
    pub(crate) start_time: Instant,
    pub(crate) pending_detection: Option<Instant>,
    pub(crate) frame_buffer: VecDeque<DynamicImage>,
    pub(crate) stats: SentryStats,
    pub(crate) active_session: Option<AlertSession>,
}

impl RoomSentry {
    pub fn new(config: SentryConfig) -> Result<Self> {
        let detector = YoloDetector::new(&config.model_path)?;
        let motion_detector = MotionDetector::new(config.motion_threshold)
            .with_ignore_top(config.ignore_top_percent)
            .with_adaptive(config.adaptive_motion);
        let telegram = TelegramClient::new(
            config.telegram_token.clone(),
            config.telegram_chat_id.clone(),
        );

        if !config.save_dir.exists() {
            std::fs::create_dir_all(&config.save_dir)?;
        }

        let pruned = prune_old_evidence(&config.save_dir, config.retention_days);
        if pruned > 0 {
            info!("Startup housekeeping: pruned {pruned} old capture(s).");
        }

        Ok(Self {
            config,
            detector,
            motion_detector,
            telegram,
            last_alert: None,
            last_person_seen: None,
            last_command_poll: Instant::now(),
            last_prune: Instant::now(),
            last_heartbeat: Instant::now(),
            armed: true,
            mute_until: None,
            start_time: Instant::now(),
            pending_detection: None,
            frame_buffer: VecDeque::with_capacity(FRAME_BUFFER_CAP),
            stats: SentryStats::new(),
            active_session: None,
        })
    }

    pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>> {
        self.stats.record_frame();
        self.push_frame_buffer(image.clone());

        let has_motion = !self.config.motion_gate || self.motion_detector.check_motion(image);
        let in_grace = self
            .last_person_seen
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3));

        if !has_motion && !in_grace {
            debug!("Static frame: skipping YOLO inference");
            self.pending_detection = None;
            return Ok(Vec::new());
        }

        let raw_detections = self.detector.detect(
            image,
            self.config.confidence_threshold,
            &self.config.targets,
        )?;

        let motion_filtered = dispatch::apply_motion_mask(
            &self.motion_detector,
            self.config.motion_gate,
            image,
            raw_detections,
        );

        let detections = dispatch::apply_watch_rect(self.config.watch_rect, image, motion_filtered);

        if detections.is_empty() {
            self.pending_detection = None;
            return Ok(Vec::new());
        }

        self.last_person_seen = Some(Instant::now());
        let peak_conf = session::peak_confidence(&detections);
        if let Some(session) = &mut self.active_session {
            session.extend(peak_conf);
        } else {
            self.active_session = Some(AlertSession::new(peak_conf));
            self.stats.record_session();
        }

        if self.is_alert_enabled() && self.temporal_confirm() {
            self.handle_alert(image, &detections)?;
        } else {
            debug!("Sentry muted/disarmed or awaiting 2-frame confirmation.");
        }

        Ok(detections)
    }

    fn push_frame_buffer(&mut self, frame: DynamicImage) {
        if self.frame_buffer.len() >= FRAME_BUFFER_CAP {
            self.frame_buffer.pop_front();
        }
        self.frame_buffer.push_back(frame);
    }

    fn temporal_confirm(&mut self) -> bool {
        match self.pending_detection {
            None => {
                self.pending_detection = Some(Instant::now());
                false
            }
            Some(first_seen) if first_seen.elapsed() <= CONFIRM_WINDOW => {
                self.pending_detection = None;
                true
            }
            Some(_) => {
                self.pending_detection = Some(Instant::now());
                false
            }
        }
    }

    fn is_alert_enabled(&self) -> bool {
        self.armed && self.mute_until.is_none_or(|until| Instant::now() >= until)
    }

    fn is_in_cooldown(&self) -> bool {
        self.last_alert
            .is_some_and(|last| last.elapsed() < Duration::from_secs(self.config.cooldown_seconds))
    }

    fn handle_alert(&mut self, image: &DynamicImage, detections: &[Detection]) -> Result<()> {
        if self.is_in_cooldown() {
            return Ok(());
        }

        self.stats.record_alert();

        let now_str = replies::ist_now_str();
        let ts = now_str.replace(" IST", "").replace(['-', ' ', ':'], "");
        let filename = format!("sentry_{ts}.jpg");
        let save_path = self.config.save_dir.join(&filename);

        let annotated = YoloDetector::annotate_frame(image, detections);
        annotated.save_with_format(&save_path, ImageFormat::Jpeg)?;

        let summary_str = dispatch::format_detection_summary(detections);
        let caption = format!(
            "🚨 <b>MONBAN ALERT — Motion Detected</b>\n\n\
            {summary_str}\n\n\
            🕒 <b>Time:</b> {now_str}\n\
            📁 <b>Evidence:</b> <code>{filename}</code>"
        );

        self.last_alert = Some(Instant::now());

        if !self.config.webhook_url.is_empty() {
            let payload = serde_json::json!({
                "event": "alert",
                "time": now_str,
                "targets": detections.len(),
                "evidence": filename,
            });
            let _ = fire_webhook(&self.config.webhook_url, &payload);
        }

        dispatch::dispatch_alert_media(&self.telegram, &self.frame_buffer, &caption)?;
        warn!(
            "🚨 Alert triggered: {} target(s) detected!",
            detections.len()
        );
        Ok(())
    }

    pub(crate) fn handle_commands(&mut self, current_frame: Option<&DynamicImage>) -> Result<()> {
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
            BotCommand::Status => replies::send_status(
                &self.telegram,
                &self.config,
                self.armed,
                self.mute_until,
                self.last_alert,
                self.start_time,
            )?,
            BotCommand::Stats => replies::send_stats(&self.telegram, &self.stats)?,
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
                Some(frame) => {
                    replies::send_snapshot(&self.telegram, &mut self.detector, &self.config, frame)?
                }
                None => {
                    self.telegram
                        .send_message("⚠️ Camera frame not yet available for snapshot.")?;
                }
            },
            BotCommand::Help => replies::send_help(&self.telegram)?,
        }
        Ok(())
    }

    fn mute_alerts(&mut self, mins: u64) -> Result<()> {
        replies::mute_alerts(&self.telegram, &mut self.mute_until, mins)
    }

    pub(crate) fn run_housekeeping(&mut self) {
        if self.last_prune.elapsed() >= PRUNE_INTERVAL {
            self.last_prune = Instant::now();
            let pruned = prune_old_evidence(&self.config.save_dir, self.config.retention_days);
            if pruned > 0 {
                info!("Periodic housekeeping: pruned {pruned} old capture(s).");
            }
        }

        if self.config.heartbeat_hours > 0
            && self.last_heartbeat.elapsed()
                >= Duration::from_secs(self.config.heartbeat_hours * 3600)
        {
            self.last_heartbeat = Instant::now();
            let heartbeat_msg = format!(
                "🟢 <b>SENTRY HEARTBEAT</b>\n\n\
                Status: <code>Active &amp; Monitoring</code>\n\
                Total Alerts: <code>{}</code>\n\
                Sessions: <code>{}</code>\n\
                Source: <code>{}</code>",
                self.stats.total_alerts, self.stats.session_count, self.config.source
            );
            let _ = self.telegram.send_message(&heartbeat_msg);
        }

        if let Some(session) = &self.active_session
            && session.is_expired()
        {
            let summary = session.summary_message();
            let _ = self.telegram.send_message(&summary);
            self.active_session = None;
        }
    }

    pub(crate) fn orient_frame(&self, frame: DynamicImage) -> DynamicImage {
        match self.config.rotate {
            90 => frame.rotate90(),
            180 => frame.rotate180(),
            270 => frame.rotate270(),
            _ => frame,
        }
    }
}
