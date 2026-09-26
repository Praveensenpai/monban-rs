use super::RoomSentry;
use crate::error::{MonbanError, Result};
use crate::infra::MjpegStream;
use std::time::{Duration, Instant};
use tracing::{info, warn};

const MAX_BACKOFF_SECS: u64 = 30;

impl RoomSentry {
    pub fn run_test(&mut self) -> Result<()> {
        info!("Running single-frame test mode on: {}", self.config.source);
        let stream = MjpegStream::connect(&self.config.source)?;
        let start = Instant::now();
        let raw_frame = loop {
            if let Ok(f) = stream.read_latest_frame() {
                break f;
            }
            if start.elapsed() > Duration::from_secs(4) {
                return Err(MonbanError::Stream(
                    "Timed out waiting for camera frame".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let frame = self.orient_frame(raw_frame);
        let test_path = self.config.save_dir.join("test_snapshot_rs.jpg");
        frame.save_with_format(&test_path, image::ImageFormat::Jpeg)?;
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
