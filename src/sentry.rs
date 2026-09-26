use crate::api::TelegramClient;
use crate::domain::{Detection, SentryConfig};
use crate::error::{MonbanError, Result};
use crate::infra::{MjpegStream, YoloDetector};
use chrono::Utc;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub struct RoomSentry {
    config: SentryConfig,
    detector: YoloDetector,
    telegram: TelegramClient,
    last_alert: Option<Instant>,
}

impl RoomSentry {
    pub fn new(config: SentryConfig) -> Result<Self> {
        let detector = YoloDetector::new(&config.model_path)?;
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
            telegram,
            last_alert: None,
        })
    }

    pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>> {
        let detections = self
            .detector
            .detect(image, self.config.confidence_threshold)?;
        if !detections.is_empty() {
            self.handle_alert(image, &detections)?;
        }
        Ok(detections)
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
        let now = Utc::now();
        let filename = format!("sentry_{}.jpg", now.format("%Y%m%d_%H%M%S"));
        let save_path = self.config.save_dir.join(&filename);

        annotated.save_with_format(&save_path, ImageFormat::Jpeg)?;

        let mut jpeg_bytes = Vec::new();
        annotated.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)?;

        let max_conf = detections
            .iter()
            .map(|d| d.confidence)
            .fold(0.0f32, f32::max);

        let caption = format!(
            "🚨 <b>MONBAN-RS ALERT — Room Sentry</b>\n\n\
            👤 <b>Intruders Detected:</b> {}\n\
            🎯 <b>Peak Confidence:</b> {:.1}%\n\
            🕒 <b>Time:</b> {}\n\
            📁 <b>Evidence:</b> <code>{}</code>",
            detections.len(),
            max_conf * 100.0,
            now.format("%Y-%m-%d %H:%M:%S UTC"),
            filename
        );

        let _ = self.telegram.send_photo_alert(jpeg_bytes, &caption);
        self.last_alert = Some(Instant::now());
        warn!(
            "🚨 Alert triggered: {} intruder(s) detected!",
            detections.len()
        );
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
            "Test frame processed. Found {} person(s).",
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
                }
                Err(MonbanError::Stream(_)) => {
                    // Frame not ready yet, wait briefly
                }
                Err(e) => return Err(e),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
