use monban_rs::domain::{BoundingBox, ConfigOverrides, Detection, SentryConfig};

#[test]
fn test_bounding_box_area_and_iou() {
    let b1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(b1.area(), 100.0);

    let b2 = BoundingBox::new(5.0, 0.0, 15.0, 10.0);
    assert_eq!(b2.area(), 100.0);

    // Overlap: 5.0 * 10.0 = 50.0. Union: 100 + 100 - 50 = 150. IoU: 50 / 150 = 1/3
    let iou = b1.iou(&b2);
    assert!((iou - (1.0 / 3.0)).abs() < 1e-5);
}

#[test]
fn test_detection_structure() {
    let det = Detection {
        class_id: 0,
        label: "person".to_string(),
        confidence: 0.95,
        box_coords: BoundingBox::new(10.0, 20.0, 100.0, 200.0),
    };
    assert_eq!(det.class_id, 0);
    assert_eq!(det.confidence, 0.95);
    assert_eq!(det.label, "person");
}

#[test]
fn test_sentry_config_defaults() {
    let config = SentryConfig::load_with_defaults(ConfigOverrides::default());
    assert_eq!(config.source, "http://192.168.1.36:4747/video");
    assert_eq!(config.confidence_threshold, 0.45);
    assert_eq!(config.cooldown_seconds, 5);
    assert!(config.motion_gate);
    assert!((config.motion_threshold - 0.005).abs() < 1e-5);
    assert!(config.targets.iter().any(|t| t == "person"));
    assert!(config.targets.iter().any(|t| t == "dog"));
    assert!(!config.targets.iter().any(|t| t == "cat"));
}

#[test]
fn test_ist_timestamp_offset() {
    use chrono::{FixedOffset, Utc};
    let ist = match FixedOffset::east_opt(19800) {
        Some(offset) => offset,
        None => panic!("Valid IST offset"),
    };
    let now = Utc::now().with_timezone(&ist);
    let formatted = now.format("%Y-%m-%d %H:%M:%S IST").to_string();
    assert!(formatted.ends_with("IST"));
}

#[test]
fn test_coco_classes_and_emojis() {
    use monban_rs::domain::{COCO_CLASSES, class_emoji};

    assert_eq!(COCO_CLASSES.len(), 80);
    assert_eq!(COCO_CLASSES[0], "person");
    assert_eq!(COCO_CLASSES[15], "cat");
    assert_eq!(COCO_CLASSES[16], "dog");
    assert_eq!(COCO_CLASSES[19], "cow");

    assert_eq!(class_emoji("person"), "👤");
    assert_eq!(class_emoji("dog"), "🐕");
    assert_eq!(class_emoji("cat"), "🐈");
    assert_eq!(class_emoji("cow"), "🐄");
    assert_eq!(class_emoji("laptop"), "💻");
    assert_eq!(class_emoji("alien_creature"), "🎯");
}

#[test]
fn test_default_target_classes() {
    use monban_rs::domain::{DEFAULT_TARGET_CLASSES, is_default_target};

    assert_eq!(DEFAULT_TARGET_CLASSES.len(), 3);
    assert!(DEFAULT_TARGET_CLASSES.contains(&"person"));
    assert!(DEFAULT_TARGET_CLASSES.contains(&"dog"));
    assert!(DEFAULT_TARGET_CLASSES.contains(&"cow"));

    assert!(is_default_target("person"));
    assert!(is_default_target("dog"));
    assert!(is_default_target("cow"));

    // Verify cat, car, airplane, bottle, chair are rejected by default
    assert!(!DEFAULT_TARGET_CLASSES.contains(&"cat"));
    assert!(!DEFAULT_TARGET_CLASSES.contains(&"car"));
    assert!(!is_default_target("cat"));
    assert!(!is_default_target("car"));
    assert!(!is_default_target("airplane"));
    assert!(!is_default_target("bottle"));
    assert!(!is_default_target("chair"));
    assert!(!is_default_target("potted plant"));
}

#[test]
fn test_yolo_detector_adaptive_input_size() {
    use image::{DynamicImage, RgbImage};
    use monban_rs::infra::YoloDetector;
    use std::path::Path;

    let dylib = Path::new("/home/paisen/.local/lib/libonnxruntime.so");
    if dylib.exists() {
        let _ = ort::init_from(dylib);
    }

    let model_path = Path::new("yolov8n.onnx");
    if model_path.exists() {
        let mut detector = match YoloDetector::new(model_path) {
            Ok(d) => d,
            Err(_) => return,
        };
        assert_eq!(detector.input_size(), 640);

        let img = DynamicImage::ImageRgb8(RgbImage::new(640, 480));
        let results = detector.detect(&img, 0.45, &[]);
        assert!(results.is_ok());

        // Test false positive rejection on real captures if present
        let targets = vec!["person".to_string(), "dog".to_string(), "cow".to_string()];
        let cap1 = Path::new("/home/paisen/captures/sentry_20260926_214730.jpg");
        if let Ok(img1) = image::open(cap1) {
            let res1 = detector.detect(&img1, 0.45, &targets).unwrap();
            // Airplane false positive MUST be completely filtered out
            assert_eq!(
                res1.len(),
                0,
                "Expected 0 detections on airplane false positive capture, got {:?}",
                res1
            );
        }

        let cap2 = Path::new("/home/paisen/captures/sentry_20260926_214703.jpg");
        if let Ok(img2) = image::open(cap2) {
            let res2 = detector.detect(&img2, 0.45, &targets).unwrap();
            // Water dispenser can scoring 0.268 MUST be filtered out by 0.45 threshold
            assert_eq!(
                res2.len(),
                0,
                "Expected 0 detections on water bottle false positive capture, got {:?}",
                res2
            );
        }

        let cap_water = Path::new("/home/paisen/captures/sentry_20260926_220538.jpg");
        if let Ok(img_water) = image::open(cap_water) {
            let upright = img_water.rotate270();
            let res_water = detector.detect(&upright, 0.45, &targets).unwrap();
            assert_eq!(
                res_water.len(),
                0,
                "Expected 0 detections on upright water can, got {:?}",
                res_water
            );
        }

        let cap_chair = Path::new("/home/paisen/captures/sentry_20260926_220614.jpg");
        if let Ok(img_chair) = image::open(cap_chair) {
            let upright = img_chair.rotate270();
            let res_chair = detector.detect(&upright, 0.45, &targets).unwrap();
            assert!(
                !res_chair.iter().any(|d| d.label == "cat"),
                "Cat must never be detected"
            );
        }
    }
}
