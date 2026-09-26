use crate::api::TelegramClient;
use crate::domain::Detection;
use crate::error::Result;
use crate::infra::video::encode_video_clip;
use crate::infra::{MotionDetector, YoloDetector, encode_animated_gif};
use image::{DynamicImage, ImageFormat};
use std::collections::VecDeque;
use std::io::Cursor;
use tracing::warn;

const VIDEO_FPS: u32 = 10;
const GIF_FRAME_DELAY_MS: u32 = 100;

/// Discard detections whose bounding-box has no overlapping motion pixels.
pub fn apply_motion_mask(
    motion_detector: &MotionDetector,
    motion_gate: bool,
    image: &DynamicImage,
    detections: Vec<Detection>,
) -> Vec<Detection> {
    if !motion_gate {
        return detections;
    }
    let (w, h) = (image.width() as f32, image.height() as f32);
    detections
        .into_iter()
        .filter(|d| motion_detector.has_motion_in_box(&d.box_coords, w, h))
        .collect()
}

/// Discard detections whose bbox centre falls outside the optional inclusion zone.
/// `watch_rect` = [x1, y1, x2, y2] as 0.0–1.0 fractions of frame dimensions.
pub fn apply_watch_rect(
    watch_rect: Option<[f32; 4]>,
    image: &DynamicImage,
    detections: Vec<Detection>,
) -> Vec<Detection> {
    let Some([rx1, ry1, rx2, ry2]) = watch_rect else {
        return detections;
    };
    let (w, h) = (image.width() as f32, image.height() as f32);
    detections
        .into_iter()
        .filter(|d| {
            let cx = (d.box_coords.x1 + d.box_coords.x2) / 2.0 / w;
            let cy = (d.box_coords.y1 + d.box_coords.y2) / 2.0 / h;
            cx >= rx1 && cx <= rx2 && cy >= ry1 && cy <= ry2
        })
        .collect()
}

pub fn format_detection_summary(detections: &[Detection]) -> String {
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

/// Try MP4 → GIF → static JPEG dispatch in priority order.
pub fn dispatch_alert_media(
    telegram: &TelegramClient,
    frame_buffer: &VecDeque<DynamicImage>,
    caption: &str,
) -> Result<()> {
    let frames: Vec<DynamicImage> = frame_buffer.iter().cloned().collect();

    if frames.len() >= 4 {
        if let Ok(mp4) = encode_video_clip(&frames, VIDEO_FPS) {
            let _ = telegram.send_video_alert(mp4, caption);
            return Ok(());
        }
        match encode_animated_gif(&frames, GIF_FRAME_DELAY_MS) {
            Ok(gif) => {
                let _ = telegram.send_animation_alert(gif, caption);
                return Ok(());
            }
            Err(e) => warn!("GIF encode failed, falling back to JPEG: {e}"),
        }
    }

    if let Some(frame) = frame_buffer.back() {
        let annotated = YoloDetector::annotate_frame(frame, &[]);
        let mut jpeg = Vec::new();
        annotated
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .ok();
        let _ = telegram.send_photo_alert(jpeg, caption);
    }
    Ok(())
}
