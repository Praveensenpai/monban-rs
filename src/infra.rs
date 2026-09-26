pub mod detector;
pub mod mjpeg;
pub mod motion;
pub mod setup;

pub use detector::YoloDetector;
pub use mjpeg::MjpegStream;
pub use motion::MotionDetector;
pub use setup::run_interactive_setup;
