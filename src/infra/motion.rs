use image::{DynamicImage, GrayImage};

/// Fast, low-overhead pixel-difference motion detector for gating YOLO inference.
pub struct MotionDetector {
    previous_frame: Option<GrayImage>,
    threshold: f32,
    pixel_delta_min: u8,
    target_width: u32,
    target_height: u32,
    ignore_top_percent: u32,
    motion_mask: Vec<bool>,
}

impl MotionDetector {
    pub fn new(threshold: f32) -> Self {
        Self {
            previous_frame: None,
            threshold: threshold.clamp(0.0005, 1.0),
            pixel_delta_min: 20,
            target_width: 128,
            target_height: 96,
            ignore_top_percent: 0,
            motion_mask: Vec::new(),
        }
    }

    pub fn with_ignore_top(mut self, percent: u32) -> Self {
        self.ignore_top_percent = percent.min(90);
        self
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
        self.motion_mask.resize(total_pixels, false);
        self.motion_mask.fill(false);

        let ignore_rows = (self.target_height * self.ignore_top_percent / 100) as usize;
        let mut changed_pixels = 0usize;
        let mut active_monitored_pixels = 0usize;

        let raw_curr = current_thumb.as_raw();
        let raw_prev = prev.as_raw();

        for y in 0..self.target_height as usize {
            let row_ignored = y < ignore_rows;
            for x in 0..self.target_width as usize {
                let idx = y * self.target_width as usize + x;
                let is_diff = raw_curr[idx].abs_diff(raw_prev[idx]) >= self.pixel_delta_min;
                if is_diff && !row_ignored {
                    self.motion_mask[idx] = true;
                    changed_pixels += 1;
                }
                if !row_ignored {
                    active_monitored_pixels += 1;
                }
            }
        }

        self.previous_frame = Some(current_thumb);
        if active_monitored_pixels == 0 {
            return false;
        }
        let ratio = changed_pixels as f32 / active_monitored_pixels as f32;
        ratio >= self.threshold
    }

    /// Checks whether a given bounding box in original image coordinates overlaps with active motion.
    pub fn has_motion_in_box(
        &self,
        box_coords: &crate::domain::BoundingBox,
        orig_w: f32,
        orig_h: f32,
    ) -> bool {
        if self.motion_mask.is_empty() || orig_w <= 0.0 || orig_h <= 0.0 {
            return true;
        }

        let gx1 = ((box_coords.x1 / orig_w) * self.target_width as f32)
            .clamp(0.0, (self.target_width - 1) as f32) as usize;
        let gy1 = ((box_coords.y1 / orig_h) * self.target_height as f32)
            .clamp(0.0, (self.target_height - 1) as f32) as usize;
        let gx2 = ((box_coords.x2 / orig_w) * self.target_width as f32)
            .clamp(0.0, (self.target_width - 1) as f32) as usize;
        let gy2 = ((box_coords.y2 / orig_h) * self.target_height as f32)
            .clamp(0.0, (self.target_height - 1) as f32) as usize;

        for y in gy1..=gy2 {
            for x in gx1..=gx2 {
                let idx = y * self.target_width as usize + x;
                if self.motion_mask.get(idx).copied().unwrap_or(false) {
                    return true;
                }
            }
        }
        false
    }

    pub fn reset(&mut self) {
        self.previous_frame = None;
        self.motion_mask.clear();
    }
}
