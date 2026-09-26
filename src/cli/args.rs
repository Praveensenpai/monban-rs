use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "monban",
    author = "paisen",
    version = "0.1.1",
    about = "🥋 門番 (Monban) — Zero-Bloat Rust AI Room & Door Sentry"
)]
pub struct CliArgs {
    #[arg(
        short,
        long,
        default_value = "http://192.168.1.36:4747/video",
        help = "Video stream URL (http://...)"
    )]
    pub source: String,

    #[arg(
        short,
        long,
        help = "Path to YOLOv8 ONNX model (defaults to yolov8n.onnx or ~/.local/share/monban/yolov8n.onnx)"
    )]
    pub model: Option<PathBuf>,

    #[arg(
        short,
        long,
        default_value_t = 0.35,
        help = "Detection confidence threshold (0.0 - 1.0)"
    )]
    pub confidence: f32,

    #[arg(long, default_value_t = 5, help = "Seconds between Telegram alerts")]
    pub cooldown: u64,

    #[arg(
        long,
        default_value = "captures",
        help = "Directory to save snapshot evidence"
    )]
    pub save_dir: PathBuf,

    #[arg(long, help = "Test mode: capture 1 frame, check detection, and exit")]
    pub test: bool,

    #[arg(short, long, help = "Enable verbose debug logging")]
    pub verbose: bool,
}
