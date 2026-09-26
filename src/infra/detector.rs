use crate::domain::{BoundingBox, Detection};
use crate::error::{MonbanError, Result};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use ndarray::Array4;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use std::path::Path;

const MODEL_INPUT_SIZE: u32 = 416;
const PERSON_CLASS_INDEX: usize = 0;
const NMS_IOU_THRESHOLD: f32 = 0.45;

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
        let input_array = Self::preprocess(image);
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
            threshold,
        )?;

        Ok(Self::apply_nms(candidates, NMS_IOU_THRESHOLD))
    }

    fn preprocess(image: &DynamicImage) -> Array4<f32> {
        let resized = image.resize_exact(
            MODEL_INPUT_SIZE,
            MODEL_INPUT_SIZE,
            image::imageops::FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();

        let mut array =
            Array4::<f32>::zeros((1, 3, MODEL_INPUT_SIZE as usize, MODEL_INPUT_SIZE as usize));
        for (x, y, pixel) in rgb.enumerate_pixels() {
            let [r, g, b] = pixel.0;
            array[[0, 0, y as usize, x as usize]] = r as f32 / 255.0;
            array[[0, 1, y as usize, x as usize]] = g as f32 / 255.0;
            array[[0, 2, y as usize, x as usize]] = b as f32 / 255.0;
        }
        array
    }

    fn parse_output(
        shape: &[i64],
        data: &[f32],
        orig_w: f32,
        orig_h: f32,
        threshold: f32,
    ) -> Result<Vec<Detection>> {
        if shape.len() != 3 {
            return Err(MonbanError::Stream(
                "Unexpected output tensor shape".to_string(),
            ));
        }

        let num_anchors = shape[2] as usize;
        let scale_x = orig_w / MODEL_INPUT_SIZE as f32;
        let scale_y = orig_h / MODEL_INPUT_SIZE as f32;

        let mut candidates = Vec::new();
        for i in 0..num_anchors {
            let person_score = data[4 * num_anchors + i];
            if person_score >= threshold {
                let cx = data[i] * scale_x;
                let cy = data[num_anchors + i] * scale_y;
                let w = data[2 * num_anchors + i] * scale_x;
                let h = data[3 * num_anchors + i] * scale_y;

                let x1 = (cx - w / 2.0).max(0.0).min(orig_w);
                let y1 = (cy - h / 2.0).max(0.0).min(orig_h);
                let x2 = (cx + w / 2.0).max(0.0).min(orig_w);
                let y2 = (cy + h / 2.0).max(0.0).min(orig_h);

                candidates.push(Detection {
                    class_id: PERSON_CLASS_INDEX,
                    label: "person".to_string(),
                    confidence: person_score,
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
        rgb
    }
}
