pub mod detector;
pub mod gif;
pub mod mjpeg;
pub mod motion;
pub mod setup;
pub mod storage;
pub mod video;
pub mod webhook;

pub use detector::YoloDetector;
pub use gif::encode_animated_gif;
pub use mjpeg::MjpegStream;
pub use motion::MotionDetector;
pub use setup::run_interactive_setup;
pub use storage::prune_old_evidence;
pub use video::encode_video_clip;
pub use webhook::fire_webhook;
