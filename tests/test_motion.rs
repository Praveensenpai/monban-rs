use image::{DynamicImage, Rgb, RgbImage};
use monban_rs::infra::MotionDetector;

#[test]
fn test_motion_detector_static_and_dynamic() {
    let mut detector = MotionDetector::new(0.015);

    // Frame 1: Pure black image (baseline established, returns true)
    let img1 = DynamicImage::ImageRgb8(RgbImage::new(100, 100));
    assert!(detector.check_motion(&img1));

    // Frame 2: Identical black image (no motion, returns false)
    let img2 = DynamicImage::ImageRgb8(RgbImage::new(100, 100));
    assert!(!detector.check_motion(&img2));

    // Frame 3: Image with 50% white pixels (large motion, returns true)
    let mut img3_raw = RgbImage::new(100, 100);
    for x in 0..50 {
        for y in 0..100 {
            img3_raw.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }
    let img3 = DynamicImage::ImageRgb8(img3_raw);
    assert!(detector.check_motion(&img3));

    // Frame 4: Identical frame again (motion stops, returns false)
    assert!(!detector.check_motion(&img3));
}

#[test]
fn test_motion_detector_subtle_distant_motion() {
    let mut detector = MotionDetector::new(0.005);
    let img1 = DynamicImage::ImageRgb8(RgbImage::new(128, 96));
    assert!(detector.check_motion(&img1)); // Baseline

    // Alter ~1% of pixels (120 pixels out of 12288)
    let mut img2_raw = RgbImage::new(128, 96);
    for x in 0..12 {
        for y in 0..10 {
            img2_raw.put_pixel(x, y, Rgb([200, 200, 200]));
        }
    }
    let img2 = DynamicImage::ImageRgb8(img2_raw);
    assert!(detector.check_motion(&img2));
}
