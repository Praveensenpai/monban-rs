use crate::domain::{BoundingBox, Detection};
use crate::error::{MonbanError, Result};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use ndarray::Array4;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use std::path::Path;

const MODEL_INPUT_SIZE: u32 = 640;
const NMS_IOU_THRESHOLD: f32 = 0.45;

#[derive(Clone, Copy)]
struct LetterboxInfo {
    scale: f32,
    pad_x: f32,
    pad_y: f32,
}

impl LetterboxInfo {
    fn compute(orig_w: u32, orig_h: u32, target: u32) -> (Self, u32, u32) {
        let scale = (target as f32 / orig_w as f32).min(target as f32 / orig_h as f32);
        let new_w = (orig_w as f32 * scale).round().clamp(1.0, target as f32) as u32;
        let new_h = (orig_h as f32 * scale).round().clamp(1.0, target as f32) as u32;
        let pad_x = (target.saturating_sub(new_w) as f32) / 2.0;
        let pad_y = (target.saturating_sub(new_h) as f32) / 2.0;
        (
            Self {
                scale,
                pad_x,
                pad_y,
            },
            new_w,
            new_h,
        )
    }
}

pub struct YoloDetector {
    session: Session,
}

impl YoloDetector {
    pub fn new(model_path: &Path) -> Result<Self> {
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(2)?
            .commit_from_file(model_path)?;

        Ok(Self { session })
    }

    pub fn detect(&mut self, image: &DynamicImage, threshold: f32) -> Result<Vec<Detection>> {
        let (orig_w, orig_h) = image.dimensions();
        let (input_array, letterbox) = Self::preprocess(image);
        let input_tensor = Tensor::from_array(input_array)?;

        let outputs = self.session.run(ort::inputs![input_tensor])?;
        let output_value = outputs
            .get("output0")
            .ok_or_else(|| MonbanError::Stream("Missing model output tensor".to_string()))?;

        let (shape, data) = output_value.try_extract_tensor::<f32>()?;
        let candidates = Self::parse_output(
            shape.as_ref(),
            data,
            orig_w as f32,
            orig_h as f32,
            &letterbox,
            threshold,
        )?;

        Ok(Self::apply_nms(candidates, NMS_IOU_THRESHOLD))
    }

    fn preprocess(image: &DynamicImage) -> (Array4<f32>, LetterboxInfo) {
        let (orig_w, orig_h) = image.dimensions();
        let (info, new_w, new_h) = LetterboxInfo::compute(orig_w, orig_h, MODEL_INPUT_SIZE);

        let resized = image.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
        let rgb = resized.to_rgb8();

        let mut array =
            Array4::<f32>::zeros((1, 3, MODEL_INPUT_SIZE as usize, MODEL_INPUT_SIZE as usize));
        let offset_x = info.pad_x.round() as usize;
        let offset_y = info.pad_y.round() as usize;

        for (x, y, pixel) in rgb.enumerate_pixels() {
            let [r, g, b] = pixel.0;
            let target_y = offset_y + y as usize;
            let target_x = offset_x + x as usize;
            if target_y < MODEL_INPUT_SIZE as usize && target_x < MODEL_INPUT_SIZE as usize {
                array[[0, 0, target_y, target_x]] = r as f32 / 255.0;
                array[[0, 1, target_y, target_x]] = g as f32 / 255.0;
                array[[0, 2, target_y, target_x]] = b as f32 / 255.0;
            }
        }

        (array, info)
    }

    fn parse_output(
        shape: &[i64],
        data: &[f32],
        orig_w: f32,
        orig_h: f32,
        info: &LetterboxInfo,
        threshold: f32,
    ) -> Result<Vec<Detection>> {
        if shape.len() != 3 {
            return Err(MonbanError::Stream(
                "Unexpected output tensor shape".to_string(),
            ));
        }

        let num_anchors = shape[2] as usize;
        let mut candidates = Vec::new();
        for i in 0..num_anchors {
            let mut best_class_idx = 0;
            let mut best_class_score = 0.0f32;
            for c in 0..80 {
                let score = data[(4 + c) * num_anchors + i];
                if score > best_class_score {
                    best_class_score = score;
                    best_class_idx = c;
                }
            }

            if best_class_score >= threshold {
                let box_cx = data[i];
                let box_cy = data[num_anchors + i];
                let box_w = data[2 * num_anchors + i];
                let box_h = data[3 * num_anchors + i];

                let cx = (box_cx - info.pad_x) / info.scale;
                let cy = (box_cy - info.pad_y) / info.scale;
                let w = box_w / info.scale;
                let h = box_h / info.scale;

                let x1 = (cx - w / 2.0).max(0.0).min(orig_w);
                let y1 = (cy - h / 2.0).max(0.0).min(orig_h);
                let x2 = (cx + w / 2.0).max(0.0).min(orig_w);
                let y2 = (cy + h / 2.0).max(0.0).min(orig_h);

                let label = match crate::domain::COCO_CLASSES.get(best_class_idx) {
                    Some(l) => *l,
                    None => "unknown",
                };

                candidates.push(Detection {
                    class_id: best_class_idx,
                    label: label.to_string(),
                    confidence: best_class_score,
                    box_coords: BoundingBox::new(x1, y1, x2, y2),
                });
            }
        }
        Ok(candidates)
    }

    fn apply_nms(mut detections: Vec<Detection>, iou_thresh: f32) -> Vec<Detection> {
        detections.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut keep = Vec::new();

        for det in detections {
            let overlaps = keep
                .iter()
                .any(|k: &Detection| k.box_coords.iou(&det.box_coords) > iou_thresh);
            if !overlaps {
                keep.push(det);
            }
        }
        keep
    }

    pub fn annotate_frame(image: &DynamicImage, detections: &[Detection]) -> RgbImage {
        let mut rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();
        let red = Rgb([255, 0, 0]);

        for det in detections {
            let x1 = (det.box_coords.x1 as u32).min(width.saturating_sub(1));
            let y1 = (det.box_coords.y1 as u32).min(height.saturating_sub(1));
            let x2 = (det.box_coords.x2 as u32).min(width.saturating_sub(1));
            let y2 = (det.box_coords.y2 as u32).min(height.saturating_sub(1));

            for x in x1..=x2 {
                rgb.put_pixel(x, y1, red);
                if y1 + 1 < height {
                    rgb.put_pixel(x, y1 + 1, red);
                }
                rgb.put_pixel(x, y2, red);
                if y2 > 0 {
                    rgb.put_pixel(x, y2 - 1, red);
                }
            }
            for y in y1..=y2 {
                rgb.put_pixel(x1, y, red);
                if x1 + 1 < width {
                    rgb.put_pixel(x1 + 1, y, red);
                }
                rgb.put_pixel(x2, y, red);
                if x2 > 0 {
                    rgb.put_pixel(x2 - 1, y, red);
                }
            }
        }

        if let Some(primary) = detections.first() {
            Self::overlay_zoom_crop(&mut rgb, image, primary);
        }

        rgb
    }

    fn overlay_zoom_crop(rgb: &mut RgbImage, original: &DynamicImage, primary: &Detection) {
        let (width, height) = rgb.dimensions();
        let box_w = (primary.box_coords.x2 - primary.box_coords.x1).max(10.0);
        let box_h = (primary.box_coords.y2 - primary.box_coords.y1).max(10.0);

        let margin_x = box_w * 0.15;
        let margin_y = box_h * 0.15;

        let crop_x = (primary.box_coords.x1 - margin_x).max(0.0) as u32;
        let crop_y = (primary.box_coords.y1 - margin_y).max(0.0) as u32;
        let crop_w = ((box_w + margin_x * 2.0) as u32).min(width.saturating_sub(crop_x));
        let crop_h = ((box_h + margin_y * 2.0) as u32).min(height.saturating_sub(crop_y));

        if crop_w < 10 || crop_h < 10 {
            return;
        }

        let pip_size = 200u32;
        if width <= pip_size + 40 || height <= pip_size + 40 {
            return;
        }

        let cropped = original.crop_imm(crop_x, crop_y, crop_w, crop_h);
        let thumb = cropped
            .resize_exact(pip_size, pip_size, image::imageops::FilterType::Triangle)
            .to_rgb8();

        let pip_x = width - pip_size - 15;
        let pip_y: u32 = 15;

        let white = Rgb([255, 255, 255]);
        let red = Rgb([255, 0, 0]);

        for x in (pip_x.saturating_sub(2))..=(pip_x + pip_size + 1) {
            for y in (pip_y.saturating_sub(2))..=(pip_y + pip_size + 1) {
                if (x < pip_x || x >= pip_x + pip_size || y < pip_y || y >= pip_y + pip_size)
                    && x < width
                    && y < height
                {
                    rgb.put_pixel(x, y, if (x + y) % 4 < 2 { red } else { white });
                }
            }
        }

        for (tx, ty, pixel) in thumb.enumerate_pixels() {
            let px = pip_x + tx;
            let py = pip_y + ty;
            if px < width && py < height {
                rgb.put_pixel(px, py, *pixel);
            }
        }
    }
}
