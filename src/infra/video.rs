use crate::error::{MonbanError, Result};
use image::{DynamicImage, ImageFormat};
use std::path::Path;
use std::process::Command;
use tracing::warn;

const CLIP_WIDTH: u32 = 320;
const CLIP_HEIGHT: u32 = 240;

/// Encodes a sequence of frames to MP4 via `ffmpeg` subprocess (320×240).
/// Returns `Err` if ffmpeg is not available — caller should fall back to GIF.
pub fn encode_video_clip(frames: &[DynamicImage], fps: u32) -> Result<Vec<u8>> {
    if frames.is_empty() {
        return Err(MonbanError::Stream("No frames for video encoding".into()));
    }

    let tmp_dir = std::env::temp_dir().join(format!("monban_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let result = encode_inner(frames, fps, &tmp_dir);
    if let Err(e) = std::fs::remove_dir_all(&tmp_dir) {
        warn!("Failed to clean up video temp dir: {e}");
    }
    result
}

fn encode_inner(frames: &[DynamicImage], fps: u32, tmp_dir: &Path) -> Result<Vec<u8>> {
    for (i, frame) in frames.iter().enumerate() {
        let resized = frame.resize_exact(
            CLIP_WIDTH,
            CLIP_HEIGHT,
            image::imageops::FilterType::Nearest,
        );
        resized.save_with_format(tmp_dir.join(format!("{i:04}.jpg")), ImageFormat::Jpeg)?;
    }

    let out_path = tmp_dir.join("clip.mp4");
    let input_pattern = tmp_dir.join("%04d.jpg");

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-framerate",
            &fps.to_string(),
            "-i",
            input_pattern.to_str().unwrap_or(""),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
            "-crf",
            "30",
            out_path.to_str().unwrap_or(""),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| MonbanError::Config(format!("ffmpeg not found: {e}")))?;

    if !status.success() {
        return Err(MonbanError::Config("ffmpeg encoding failed".into()));
    }

    std::fs::read(&out_path).map_err(Into::into)
}
