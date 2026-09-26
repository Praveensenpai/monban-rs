pub mod replies;

use crate::api::{BotCommand, TelegramClient};
use crate::domain::{Detection, SentryConfig};
use crate::error::{MonbanError, Result};
use crate::infra::{
    MjpegStream, MotionDetector, YoloDetector, encode_animated_gif, prune_old_evidence,
};
use image::{DynamicImage, ImageFormat};
use std::collections::VecDeque;
use std::io::Cursor;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// Rolling ring-buffer capacity for animated GIF clips (≈3s @ 10fps)
const FRAME_BUFFER_CAP: usize = 30;
// GIF frame delay: 100ms ≈ 10fps
const GIF_FRAME_DELAY_MS: u32 = 100;
// 2-frame temporal confirmation window
const CONFIRM_WINDOW: Duration = Duration::from_millis(700);
// Exponential back-off ceiling for stream reconnect
const MAX_BACKOFF_SECS: u64 = 30;
// Evidence housekeeping interval
const PRUNE_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub struct RoomSentry {
    config: SentryConfig,
    detector: YoloDetector,
    motion_detector: MotionDetector,
    telegram: TelegramClient,
    last_alert: Option<Instant>,
    last_person_seen: Option<Instant>,
    last_command_poll: Instant,
    last_prune: Instant,
    armed: bool,
    mute_until: Option<Instant>,
    start_time: Instant,
    /// Pending first-seen instant for 2-frame temporal confirmation
    pending_detection: Option<Instant>,
    /// Rolling frame ring-buffer for GIF clip generation
    frame_buffer: VecDeque<DynamicImage>,
}

impl RoomSentry {
    pub fn new(config: SentryConfig) -> Result<Self> {
        let detector = YoloDetector::new(&config.model_path)?;
        let motion_detector =
            MotionDetector::new(config.motion_threshold).with_ignore_top(config.ignore_top_percent);
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
            armed: true,
            mute_until: None,
            start_time: Instant::now(),
            pending_detection: None,
            frame_buffer: VecDeque::with_capacity(FRAME_BUFFER_CAP),
        })
    }

    pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>> {
        let has_motion = !self.config.motion_gate || self.motion_detector.check_motion(image);
        let in_grace = self
            .last_person_seen
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3));

        self.push_frame_buffer(image.clone());

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

        let detections = self.apply_motion_mask(image, raw_detections);

        if detections.is_empty() {
            self.pending_detection = None;
            return Ok(Vec::new());
        }

        self.last_person_seen = Some(Instant::now());

        if self.is_alert_enabled() && self.temporal_confirm() {
            self.handle_alert(image, &detections)?;
        } else {
            debug!("Sentry muted/disarmed or awaiting 2-frame confirmation.");
        }

        Ok(detections)
    }

    // ── private helpers ──────────────────────────────────────────────────────

    fn push_frame_buffer(&mut self, frame: DynamicImage) {
        if self.frame_buffer.len() >= FRAME_BUFFER_CAP {
            self.frame_buffer.pop_front();
        }
        self.frame_buffer.push_back(frame);
    }

    fn apply_motion_mask(
        &self,
        image: &DynamicImage,
        detections: Vec<Detection>,
    ) -> Vec<Detection> {
        if !self.config.motion_gate {
            return detections;
        }
        let orig_w = image.width() as f32;
        let orig_h = image.height() as f32;
        detections
            .into_iter()
            .filter(|d| {
                self.motion_detector
                    .has_motion_in_box(&d.box_coords, orig_w, orig_h)
            })
            .collect()
    }

    /// Returns true once target detected on 2 consecutive frames within CONFIRM_WINDOW.
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
                // Window expired — restart confirmation
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

        let now_str = replies::ist_now_str();
        let ts = now_str.replace(" IST", "").replace(['-', ' ', ':'], "");
        let filename = format!("sentry_{ts}.jpg");
        let save_path = self.config.save_dir.join(&filename);

        let annotated = YoloDetector::annotate_frame(image, detections);
        annotated.save_with_format(&save_path, ImageFormat::Jpeg)?;

        let summary_str = Self::format_detection_summary(detections);
        let caption = format!(
            "🚨 <b>MONBAN ALERT — Motion Detected</b>\n\n\
            {summary_str}\n\n\
            🕒 <b>Time:</b> {now_str}\n\
            📁 <b>Evidence:</b> <code>{filename}</code>"
        );

        self.last_alert = Some(Instant::now());
        self.dispatch_alert_media(&caption)?;
        warn!("🚨 Alert triggered: {} target(s) detected!", detections.len());
        Ok(())
    }

    /// Sends animated GIF if enough frames; falls back to static JPEG.
    fn dispatch_alert_media(&mut self, caption: &str) -> Result<()> {
        let frames: Vec<DynamicImage> = self.frame_buffer.iter().cloned().collect();
        if frames.len() >= 4 {
            match encode_animated_gif(&frames, GIF_FRAME_DELAY_MS) {
                Ok(gif_bytes) => {
                    let _ = self.telegram.send_animation_alert(gif_bytes, caption);
                    return Ok(());
                }
                Err(e) => warn!("GIF encode failed, falling back to JPEG: {e}"),
            }
        }
        // Fallback: static JPEG from latest frame
        if let Some(frame) = self.frame_buffer.back() {
            let mut jpeg_bytes = Vec::new();
            frame.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)?;
            let _ = self.telegram.send_photo_alert(jpeg_bytes, caption);
        }
        Ok(())
    }

    fn format_detection_summary(detections: &[Detection]) -> String {
        let count = detections.len();
        let max_conf = detections
            .iter()
            .map(|d| d.confidence)
            .fold(0.0f32, f32::max);
        format!(
            "🎯 <b>Target Detected:</b> {count} ({:.1}%)",
            max_conf * 100.0
        )
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
            BotCommand::Status => replies::send_status(
                &self.telegram,
                &self.config,
                self.armed,
                self.mute_until,
                self.last_alert,
                self.start_time,
            )?,
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
                Some(frame) => replies::send_snapshot(
                    &self.telegram,
                    &mut self.detector,
                    &self.config,
                    frame,
                )?,
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

    fn run_housekeeping(&mut self) {
        if self.last_prune.elapsed() >= PRUNE_INTERVAL {
            self.last_prune = Instant::now();
            let pruned = prune_old_evidence(&self.config.save_dir, self.config.retention_days);
            if pruned > 0 {
                info!("Periodic housekeeping: pruned {pruned} old capture(s).");
            }
        }
    }

    fn orient_frame(&self, frame: DynamicImage) -> DynamicImage {
        match self.config.rotate {
            90 => frame.rotate90(),
            180 => frame.rotate180(),
            270 => frame.rotate270(),
            _ => frame,
        }
    }

    pub fn run_test(&mut self) -> Result<()> {
        info!("Running single-frame test mode on: {}", self.config.source);
        let stream = MjpegStream::connect(&self.config.source)?;
        let start = Instant::now();
        let raw_frame = loop {
            if let Ok(f) = stream.read_latest_frame() { break f; }
            if start.elapsed() > Duration::from_secs(4) {
                return Err(MonbanError::Stream("Timed out waiting for camera frame".into()));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let frame = self.orient_frame(raw_frame);
        let test_path = self.config.save_dir.join("test_snapshot_rs.jpg");
        frame.save_with_format(&test_path, ImageFormat::Jpeg)?;
        info!("Saved raw test frame to {:?}", test_path);
        let detections = self.process_frame(&frame)?;
        info!("Test frame processed. Found {} target(s).", detections.len());
        Ok(())
    }

    pub fn run_loop(&mut self) -> Result<()> {
        info!("Starting Monban Sentry on: {}", self.config.source);
        let mut backoff_secs: u64 = 2;

        'outer: loop {
            let stream = match MjpegStream::connect(&self.config.source) {
                Ok(s) => {
                    if backoff_secs > 2 {
                        let _ = self
                            .telegram
                            .send_message("✅ <b>Camera reconnected.</b> Sentry resumed.");
                        info!("Camera reconnected.");
                    }
                    backoff_secs = 2;
                    s
                }
                Err(e) => {
                    warn!("Camera connect failed: {e}. Retrying in {backoff_secs}s…");
                    let _ = self.telegram.send_message(&format!(
                        "⚠️ <b>Camera disconnected.</b> Retrying in {backoff_secs}s…"
                    ));
                    std::thread::sleep(Duration::from_secs(backoff_secs));
                    backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                    continue;
                }
            };

            loop {
                match stream.read_latest_frame() {
                    Ok(raw_frame) => {
                        backoff_secs = 2;
                        let frame = self.orient_frame(raw_frame);
                        if let Err(e) = self.process_frame(&frame) {
                            warn!("Error processing frame: {e}");
                        }
                        if let Err(e) = self.handle_commands(Some(&frame)) {
                            warn!("Error handling bot commands: {e}");
                        }
                        self.run_housekeeping();
                    }
                    Err(MonbanError::Stream(_)) => {
                        if let Err(e) = self.handle_commands(None) {
                            warn!("Error handling bot commands: {e}");
                        }
                    }
                    Err(e) => {
                        warn!("Fatal stream error: {e}. Reconnecting in {backoff_secs}s…");
                        let _ = self.telegram.send_message(&format!(
                            "⚠️ <b>Camera disconnected.</b> Retrying in {backoff_secs}s…"
                        ));
                        std::thread::sleep(Duration::from_secs(backoff_secs));
                        backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                        continue 'outer;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
