# CODEBASE.md: monban-rs Semantic Digest

> **Notice**: This file is an AI-optimized semantic index. Do not write narrative prose. Keep token density high.

## 1. System Topology & Data Flow
```text
[Camera Stream: HTTP MJPEG (DroidCam/IPCam)]
                  │
                  ▼
         MjpegStream (Background Worker)
                  │ 0xFF,0xD8 .. 0xFF,0xD9 extraction
                  ▼
         RoomSentry.process_frame()
                  │
                  ▼
         YoloDetector.detect() (ONNX Runtime, 416x416)
                  │
                  ▼
         NMS (Non-Maximum Suppression)
                  │
         ┌────────┴────────┐
    [No Person]      [Person Found]
         │                 │
      Continue             ▼
                     Cooldown Check
                           │
                 ┌─────────┴─────────┐
             [Active]            [Expired]
                 │                   │
             Suppress                ▼
                               annotate_frame()
                               save evidence to disk
                                     │
                                     ▼
                          TelegramClient.send_photo_alert()
```

## 2. Global Constraints & Architecture Patterns
- **Primary Language & Edition**: Rust 2021 edition (Rust 1.97.1).
- **Architectural Paradigm**: Role-based (`domain/`, `infra/`, `api/`, `cli/`, `sentry`).
- **Hard Constraints**: <400 lines/file, <60 lines/fn, zero production `unwrap()`/`expect()`, zero compiler/clippy warnings (`-D warnings`).
- **Target Distribution**: Standalone Linux x86_64 binary (`11 MB`), **Peak RSS: ~69 MB** (down from 818 MB in Python).

## 3. Module & Interface Skeleton

### `src/error.rs` (Role: error, Lines: 30)
- **Responsibility**: Centralized domain error enumeration and `ort` / `image` / `reqwest` conversions.
- **Types & Enums**:
  ```rust
  pub enum MonbanError {
      Config(String),
      Network(reqwest::Error),
      Image(image::ImageError),
      Ort(String),
      Io(std::io::Error),
      Stream(String),
  }
  pub type Result<T> = std::result::Result<T, MonbanError>;
  ```

### `src/domain/models.rs` (Role: domain, Lines: 45)
- **Responsibility**: Pure domain models for bounding boxes, area calculation, and Intersection-over-Union (IoU).
- **Types & Enums**:
  ```rust
  pub struct BoundingBox { pub x1: f32, pub y1: f32, pub x2: f32, pub y2: f32 }
  pub struct Detection { pub class_id: usize, pub label: String, pub confidence: f32, pub box_coords: BoundingBox }
  ```
- **Public Functions & Signatures**:
  ```rust
  impl BoundingBox {
      pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self;
      pub fn area(&self) -> f32;
      pub fn iou(&self, other: &Self) -> f32;
  }
  ```

### `src/domain/config.rs` (Role: domain, Lines: 65)
- **Responsibility**: Runtime configuration with automated `~/.config/tayori/config.toml` credential discovery.
- **Types & Enums**:
  ```rust
  pub struct SentryConfig {
      pub source: String,
      pub model_path: PathBuf,
      pub confidence_threshold: f32,
      pub cooldown_seconds: u64,
      pub telegram_token: Option<String>,
      pub telegram_chat_id: Option<String>,
      pub save_dir: PathBuf,
  }
  ```
- **Public Functions & Signatures**:
  ```rust
  impl SentryConfig {
      pub fn load_with_defaults(source: Option<String>, model_path: Option<PathBuf>, confidence: Option<f32>, cooldown: Option<u64>, save_dir: Option<PathBuf>) -> Self;
  }
  ```

### `src/infra/mjpeg.rs` (Role: infra, Lines: 129)
- **Responsibility**: Background worker thread that streams multipart JPEG HTTP chunked responses with zero buffer queue lag.
- **Types & Enums**:
  ```rust
  pub struct MjpegStream { ... }
  ```
- **Public Functions & Signatures**:
  ```rust
  impl MjpegStream {
      pub fn connect(url: &str) -> Result<Self>;
      pub fn read_latest_frame(&self) -> Result<DynamicImage>;
  }
  ```

### `src/infra/detector.rs` (Role: infra, Lines: 164)
- **Responsibility**: ONNX Runtime YOLOv8n detector with letterboxing/resizing to 416x416, tensor inference, and NMS.
- **Types & Enums**:
  ```rust
  pub struct YoloDetector { ... }
  ```
- **Public Functions & Signatures**:
  ```rust
  impl YoloDetector {
      pub fn new(model_path: &Path) -> Result<Self>;
      pub fn detect(&mut self, image: &DynamicImage, threshold: f32) -> Result<Vec<Detection>>;
      pub fn annotate_frame(image: &DynamicImage, detections: &[Detection]) -> RgbImage;
  }
  ```

### `src/api/telegram.rs` (Role: api, Lines: 62)
- **Responsibility**: Dispatches HTML multipart image notifications to Telegram Bot API.
- **Public Functions & Signatures**:
  ```rust
  impl TelegramClient {
      pub fn new(token: Option<String>, chat_id: Option<String>) -> Self;
      pub fn is_configured(&self) -> bool;
      pub fn send_photo_alert(&self, image_bytes: Vec<u8>, caption: &str) -> Result<bool>;
  }
  ```

### `src/sentry.rs` (Role: sentry, Lines: 145)
- **Responsibility**: Core guardian loop coordinating stream frames, detection, cooldown filtering, disk evidence, and Telegram alerts.
- **Public Functions & Signatures**:
  ```rust
  impl RoomSentry {
      pub fn new(config: SentryConfig) -> Result<Self>;
      pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>>;
      pub fn run_test(&mut self) -> Result<()>;
      pub fn run_loop(&mut self) -> Result<()>;
  }
  ```

## 4. Execution Lifecycle Trace
1. **Startup**: `src/main.rs` ensures `ORT_DYLIB_PATH` is set, sets up `tracing_subscriber`, and parses CLI args.
2. **Config Discovery**: `SentryConfig::load_with_defaults()` discovers Telegram credentials from `~/.config/tayori/config.toml`.
3. **Model & Stream Init**: `YoloDetector` loads `yolov8n.onnx` into ONNX Runtime session; `MjpegStream` spawns background worker.
4. **Guard Loop**: Every cycle reads latest frame, runs YOLO inference (42ms), applies NMS, saves evidence on intruder, and sends photo alert.
5. **Exit**: Clean graceful teardown on SIGINT/Ctrl+C.

## 5. Verification Commands
```bash
# Lint, format, and typecheck
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# Automated test suite
cargo test --all-targets

# Live execution test
./target/release/monban-rs --test
```

## 6. Recent Iteration Changes
- **2026-09-26**: Formatted all runtime logs in IST (Indian Standard Time, UTC+05:30) via custom `tracing_subscriber` `FormatTime` timer; replaced UTC timestamps across the entire application.
- **2026-09-26**: Fixed ONNX Runtime dylib dynamic loader to canonicalize search paths and invoke `ort::init_from` explicitly; converted CLI `--model` to `Option<PathBuf>` enabling seamless global fallback to `~/.local/share/monban/yolov8n.onnx` from any working directory.
- **2026-09-26**: Reduced default alert cooldown from 30s to 5s across CLI and domain config.
- **2026-09-26**: Complete Rust port of Monban AI room sentry. 11MB standalone binary, 69MB RAM footprint (91.5% reduction), 42ms inference latency.
