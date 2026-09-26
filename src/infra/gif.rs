use crate::error::{MonbanError, Result};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, DynamicImage, Frame, RgbaImage};
use std::time::Duration;

/// Encodes a sequence of frames into an animated GIF.
pub fn encode_animated_gif(frames: &[DynamicImage], frame_delay_ms: u32) -> Result<Vec<u8>> {
    if frames.is_empty() {
        return Err(MonbanError::Stream(
            "No frames for GIF generation".to_string(),
        ));
    }

    let mut buffer = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buffer);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| MonbanError::Config(e.to_string()))?;

        let delay = Delay::from_saturating_duration(Duration::from_millis(frame_delay_ms as u64));

        for frame in frames {
            let resized = frame.resize_exact(320, 240, image::imageops::FilterType::Nearest);
            let rgba: RgbaImage = resized.to_rgba8();
            let gif_frame = Frame::from_parts(rgba, 0, 0, delay);
            encoder
                .encode_frame(gif_frame)
                .map_err(|e| MonbanError::Config(e.to_string()))?;
        }
    }

    Ok(buffer)
}
