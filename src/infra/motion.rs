use image::{DynamicImage, GrayImage};

/// Fast, low-overhead pixel-difference motion detector for gating YOLO inference.
pub struct MotionDetector {
    previous_frame: Option<GrayImage>,
    threshold: f32,
    pixel_delta_min: u8,
    target_width: u32,
    target_height: u32,
}

impl MotionDetector {
    pub fn new(threshold: f32) -> Self {
        Self {
            previous_frame: None,
            threshold: threshold.clamp(0.0005, 1.0),
            pixel_delta_min: 20,
            target_width: 128,
            target_height: 96,
        }
    }

    /// Evaluates if there is significant motion between the current and previous frame.
    /// Always returns true for the very first frame to establish a baseline.
    pub fn check_motion(&mut self, image: &DynamicImage) -> bool {
        let current_thumb = image
            .resize_exact(
                self.target_width,
                self.target_height,
                image::imageops::FilterType::Nearest,
            )
            .to_luma8();

        let prev = match &self.previous_frame {
            Some(prev) => prev,
            None => {
                self.previous_frame = Some(current_thumb);
                return true;
            }
        };

        let total_pixels = (self.target_width * self.target_height) as usize;
        let mut changed_pixels = 0usize;

        for (p1, p2) in current_thumb.as_raw().iter().zip(prev.as_raw().iter()) {
            if p1.abs_diff(*p2) >= self.pixel_delta_min {
                changed_pixels += 1;
            }
        }

        self.previous_frame = Some(current_thumb);
        let ratio = changed_pixels as f32 / total_pixels as f32;
        ratio >= self.threshold
    }

    pub fn reset(&mut self) {
        self.previous_frame = None;
    }
}
