use image::{DynamicImage, RgbImage};
use monban_rs::domain::{BoundingBox, Detection, SentryStats};
use monban_rs::infra::MotionDetector;
use monban_rs::sentry::dispatch::apply_watch_rect;
use monban_rs::sentry::session::AlertSession;

#[test]
fn test_sentry_stats_recording() {
    let mut stats = SentryStats::new();
    assert_eq!(stats.total_alerts, 0);
    assert_eq!(stats.session_count, 0);
    assert_eq!(stats.frames_processed, 0);

    stats.record_frame();
    stats.record_frame();
    assert_eq!(stats.frames_processed, 2);

    stats.record_session();
    assert_eq!(stats.session_count, 1);

    stats.record_alert();
    stats.record_alert();
    assert_eq!(stats.total_alerts, 2);

    let msg = stats.format_message();
    assert!(msg.contains("MONBAN STATS"));
    assert!(msg.contains("Alerts: <code>2</code>"));
    assert!(msg.contains("Sessions: <code>1</code>"));
}

#[test]
fn test_alert_session_lifecycle() {
    let mut session = AlertSession::new(0.65);
    assert_eq!(session.alert_count, 1);
    assert!((session.max_confidence - 0.65).abs() < f32::EPSILON);
    assert!(!session.is_expired());

    session.extend(0.85);
    assert_eq!(session.alert_count, 2);
    assert!((session.max_confidence - 0.85).abs() < f32::EPSILON);

    let summary = session.summary_message();
    assert!(summary.contains("Session ended"));
    assert!(summary.contains("85.0%"));
    assert!(summary.contains("2"));
}

#[test]
fn test_apply_watch_rect_inclusion() {
    let img = DynamicImage::ImageRgb8(RgbImage::new(100, 100));

    // Detection in center: x1=40, y1=40, x2=60, y2=60 -> center=(50, 50) -> normalized=(0.5, 0.5)
    let det_center = Detection {
        class_id: 0,
        label: "target".to_string(),
        confidence: 0.9,
        box_coords: BoundingBox::new(40.0, 40.0, 60.0, 60.0),
    };

    // Detection in corner: x1=5, y1=5, x2=15, y2=15 -> center=(10, 10) -> normalized=(0.1, 0.1)
    let det_corner = Detection {
        class_id: 0,
        label: "target".to_string(),
        confidence: 0.8,
        box_coords: BoundingBox::new(5.0, 5.0, 15.0, 15.0),
    };

    let all = vec![det_center.clone(), det_corner.clone()];

    // Watch zone only covering center [0.3, 0.3, 0.7, 0.7]
    let watch_zone = Some([0.3, 0.3, 0.7, 0.7]);
    let filtered = apply_watch_rect(watch_zone, &img, all);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].box_coords, det_center.box_coords);

    // Watch zone None -> allows all
    let unfiltered = apply_watch_rect(None, &img, vec![det_center, det_corner]);
    assert_eq!(unfiltered.len(), 2);
}

#[test]
fn test_adaptive_motion_detector() {
    let mut detector = MotionDetector::new(0.05).with_adaptive(true);
    let frame1 = DynamicImage::ImageRgb8(RgbImage::new(128, 96));
    assert!(detector.check_motion(&frame1));

    let frame2 = DynamicImage::ImageRgb8(RgbImage::new(128, 96));
    assert!(!detector.check_motion(&frame2));
}
