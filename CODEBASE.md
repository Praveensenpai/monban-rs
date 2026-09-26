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

### `src/domain/models.rs` (Role: domain, Lines: 146)
- **Responsibility**: Pure domain models for bounding boxes, area calculation, Intersection-over-Union (IoU), all 80 COCO classes, and emoji mapping.
- **Types & Enums**:
  ```rust
  pub struct BoundingBox { pub x1: f32, pub y1: f32, pub x2: f32, pub y2: f32 }
  pub struct Detection { pub class_id: usize, pub label: String, pub confidence: f32, pub box_coords: BoundingBox }
  pub const COCO_CLASSES: [&str; 80] = [ ... ];
  ```
- **Public Functions & Signatures**:
  ```rust
  impl BoundingBox {
      pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self;
      pub fn area(&self) -> f32;
      pub fn iou(&self, other: &Self) -> f32;
  }
  pub fn class_emoji(label: &str) -> &'static str;
  ```

### `src/domain/config.rs` (Role: domain, Lines: 155)
- **Responsibility**: Runtime configuration with priority: CLI > `~/.config/monban/config.toml` > `~/.config/tayori/config.toml`.
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
      pub motion_gate: bool,
      pub motion_threshold: f32,
  }
  ```

### `src/infra/motion.rs` (Role: infra, Lines: 59)
- **Responsibility**: Ultra-fast grayscale pixel-difference motion detector gating YOLO inference (128x96 grid, sub-0.1ms).
- **Public Functions & Signatures**:
  ```rust
  impl MotionDetector {
      pub fn new(threshold: f32) -> Self;
      pub fn check_motion(&mut self, image: &DynamicImage) -> bool;
      pub fn reset(&mut self);
  }
  ```

### `src/infra/setup.rs` (Role: infra, Lines: 154)
- **Responsibility**: Interactive terminal setup wizard configuring and validating dedicated Telegram bot.
- **Public Functions & Signatures**:
  ```rust
  pub fn run_interactive_setup() -> Result<()>;
  ```

### `src/infra/mjpeg.rs` (Role: infra, Lines: 131)
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

### `src/infra/detector.rs` (Role: infra, Lines: 273)
- **Responsibility**: ONNX Runtime YOLOv8n detector with letterboxing to native 640x640, 80-class scanning, NMS, and Picture-in-Picture (PiP) zoom thumbnail overlay.
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

### `src/api/telegram.rs` (Role: api, Lines: 236)
- **Responsibility**: Two-way Telegram bot integration with interactive inline button keyboards (`[📸 Snapshot]`, `[🛡️ Mute 10m]`, `[⚔️ Arm]`, `[🛑 Disarm]`), toast callback queries, command polling (`/status`, `/snap`, `/mute`, `/arm`, `/disarm`, `/help`), and multipart photo alerts.
- **Public Functions & Signatures**:
  ```rust
  pub enum BotCommand { Status, Arm, Disarm, Mute(u64), Snap, Help }
  impl TelegramClient {
      pub fn new(token: Option<String>, chat_id: Option<String>) -> Self;
      pub fn is_configured(&self) -> bool;
      pub fn send_message(&self, text: &str) -> Result<bool>;
      pub fn send_photo_alert(&self, image_bytes: Vec<u8>, caption: &str) -> Result<bool>;
      pub fn answer_callback(&self, query_id: &str, toast: &str) -> Result<()>;
      pub fn poll_commands(&mut self) -> Result<Vec<BotCommand>>;
  }
  ```

### `src/sentry.rs` (Role: sentry, Lines: 382)
- **Responsibility**: Core guardian loop coordinating stream frames, motion gating, multi-object detection, cooldown filtering, PiP zoom evidence, temporary mute timers, and Telegram commands/buttons.
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
1. **Startup**: `src/main.rs` parses CLI args. If `--setup`, runs interactive terminal wizard and exits.
2. **Dynamic Dylib**: Resolves `libonnxruntime.so` to absolute canonical path and calls `ort::init_from`.
3. **Config Discovery**: Priority: CLI > `~/.config/monban/config.toml` > `~/.config/tayori/config.toml`.
4. **Guard Loop**: Every cycle reads latest frame, checks motion detector (<0.1ms). If static, skips YOLO inference (idle CPU <2%). If motion or within 3s grace window, runs YOLO (42ms).
5. **Two-Way Control**: Sentry polls authorized Telegram commands every 1.5s (`/status`, `/snap`, `/arm`, `/disarm`, `/help`).

## 5. Verification Commands
```bash
# Lint, format, and typecheck
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# Automated test suite
cargo test --all-targets

# Interactive dedicated bot setup
monban --setup

# Live execution test
monban --test
```

## 6. Recent Iteration Changes
- **2026-09-26 (v0.3.0)**: Added multi-object classification across all 80 COCO classes with contextual emojis (`COCO_CLASSES`, `class_emoji`). Added Picture-in-Picture (PiP) zoom thumbnail overlay on detected targets. Added interactive Telegram inline buttons (`[📸 Snapshot]`, `[🛡️ Mute 10m]`, `[⚔️ Arm]`, `[🛑 Disarm]`) with instant toast replies and timed mute alerts (`/mute`).
- **2026-09-26**: Upgraded YOLO input resolution to native 640×640 with aspect-ratio preserving letterboxing (2.37× pixel density increase) and tuned confidence to 0.25 for distant intruder detection across rooms; upgraded motion grid to 128×96 with 0.005 sensitivity threshold.
- **2026-09-26**: Added ultra-low-overhead pixel difference `MotionDetector` gating (idle CPU drops from ~150% to <2%) with 3s intruder grace period.
- **2026-09-26**: Added two-way Telegram bot command control (`/status`, `/snap`, `/arm`, `/disarm`, `/help`) with strict `chat_id` authentication.
- **2026-09-26**: Added `monban --setup` interactive terminal setup wizard for dedicated Telegram bot configuration stored in `~/.config/monban/config.toml`.
- **2026-09-26**: Formatted all runtime logs in IST (Indian Standard Time, UTC+05:30) via custom `tracing_subscriber` `FormatTime` timer; replaced UTC timestamps across the entire application.
- **2026-09-26**: Fixed ONNX Runtime dylib dynamic loader to canonicalize search paths and invoke `ort::init_from` explicitly; converted CLI `--model` to `Option<PathBuf>` enabling seamless global fallback to `~/.local/share/monban/yolov8n.onnx` from any working directory.
- **2026-09-26**: Reduced default alert cooldown from 30s to 5s across CLI and domain config.
- **2026-09-26**: Complete Rust port of Monban AI room sentry. 11MB standalone binary, 69MB RAM footprint (91.5% reduction), 42ms inference latency.
