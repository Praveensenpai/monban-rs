# CODEBASE.md: monban-rs Semantic Digest

> **Notice**: This file is an AI-optimized semantic index. Do not write narrative prose. Keep token density high.

## 1. System Topology & Data Flow
```text
[Camera Stream: HTTP MJPEG (DroidCam/IPCam)]   <── Auto-Reconnect (exp backoff 2→30s)
                  │
                  ▼
         MjpegStream (Background Worker)
                  │ 0xFF,0xD8 .. 0xFF,0xD9 extraction
                  ▼
         FrameBuffer (VecDeque<DynamicImage>, cap=30)  <── Rolling ring-buffer for GIF
                  │
                  ▼
         MotionDetector (128x96 grayscale, ignore_top_percent)
                  │ check_motion() -> motion_mask: Vec<bool>
                  ▼
         RoomSentry.process_frame()
                  │
                  ▼
         YoloDetector.detect() (ONNX Runtime, 416x416 / 640x640)
                  │
                  ▼
         apply_motion_mask() -> has_motion_in_box() per bbox
                  │
                  ▼
         temporal_confirm() -> 2-frame window (600ms)
                  │
          ┌───────┴───────┐
     [No Confirm]  [Confirmed]
          │               │
       Continue            ▼
                      Cooldown Check
                           │
               ┌───────────┴───────────┐
           [Active]               [Expired]
               │                       │
           Suppress              annotate_frame()
                                save evidence to disk
                                       │
                                       ▼
                          encode_animated_gif() (320x240 @ 10fps)
                                       │
                         ┌─────────────┴─────────────┐
                    [GIF OK]                     [Fallback]
                         │                           │
                  sendAnimation              send_photo_alert()
                  (Telegram Bot API)          (static JPEG)
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

### `src/domain/models.rs` (Role: domain, Lines: 154)
- **Responsibility**: Pure domain models for bounding boxes, area calculation, Intersection-over-Union (IoU), 80 COCO classes, emoji mapping, and target guardian class filtering.
- **Types & Enums**:
  ```rust
  pub struct BoundingBox { pub x1: f32, pub y1: f32, pub x2: f32, pub y2: f32 }
  pub struct Detection { pub class_id: usize, pub label: String, pub confidence: f32, pub box_coords: BoundingBox }
  pub const COCO_CLASSES: [&str; 80] = [ ... ];
  pub const DEFAULT_TARGET_CLASSES: [&str; 3] = [ "person", "dog", "cow" ];
  ```
- **Public Functions & Signatures**:
  ```rust
  impl BoundingBox {
      pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self;
      pub fn area(&self) -> f32;
      pub fn iou(&self, other: &Self) -> f32;
  }
  pub fn class_emoji(label: &str) -> &'static str;
  pub fn is_default_target(label: &str) -> bool;
  ```

### `src/domain/config.rs` (Role: domain, Lines: ~190)
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
      pub targets: Vec<String>,
      pub rotate: u32,
      pub ignore_top_percent: u32,   // NEW: ROI exclusion (0–90%)
      pub retention_days: u32,       // NEW: evidence retention (default 7)
  }
  pub struct ConfigOverrides {
      pub source: Option<String>,
      pub model_path: Option<PathBuf>,
      pub confidence: Option<f32>,
      pub cooldown: Option<u64>,
      pub save_dir: Option<PathBuf>,
      pub motion_gate: Option<bool>,
      pub motion_threshold: Option<f32>,
      pub targets: Option<Vec<String>>,
      pub rotate: Option<u32>,
      pub ignore_top_percent: Option<u32>,  // NEW
      pub retention_days: Option<u32>,      // NEW
  }
  ```

### `src/infra/motion.rs` (Role: infra, Lines: 121)
- **Responsibility**: Ultra-fast grayscale pixel-difference motion detector gating YOLO inference (128x96 grid, sub-0.1ms). Now includes per-pixel motion mask and ROI top-exclusion.
- **Public Functions & Signatures**:
  ```rust
  impl MotionDetector {
      pub fn new(threshold: f32) -> Self;
      pub fn with_ignore_top(self, percent: u32) -> Self;  // NEW
      pub fn check_motion(&mut self, image: &DynamicImage) -> bool;
      pub fn has_motion_in_box(&self, bbox: &BoundingBox, orig_w: f32, orig_h: f32) -> bool;  // NEW
      pub fn reset(&mut self);
  }
  ```

### `src/infra/gif.rs` (Role: infra, Lines: 35)
- **Responsibility**: Encodes a sequence of `DynamicImage` frames into an animated GIF at 320x240 for Telegram sendAnimation dispatch.
- **Public Functions & Signatures**:
  ```rust
  pub fn encode_animated_gif(frames: &[DynamicImage], frame_delay_ms: u32) -> Result<Vec<u8>>;
  ```

### `src/infra/storage.rs` (Role: infra, Lines: 59)
- **Responsibility**: Evidence retention housekeeping — scans save_dir and deletes `sentry_*.jpg` files older than `retention_days` days.
- **Public Functions & Signatures**:
  ```rust
  pub fn prune_old_evidence(save_dir: &Path, retention_days: u32) -> usize;
  ```

### `src/infra/setup.rs` (Role: infra, Lines: 154)
- **Responsibility**: Interactive terminal setup wizard configuring and validating dedicated Telegram bot.
- **Public Functions & Signatures**:
  ```rust
  pub fn run_interactive_setup() -> Result<()>;
  ```

### `src/infra/mjpeg.rs` (Role: infra, Lines: 134)
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

### `src/infra/detector.rs` (Role: infra, Lines: 294)
- **Responsibility**: ONNX Runtime YOLOv8n detector with dynamic model input resolution auto-detection (supports both 416x416 and 640x640), aspect-ratio letterboxing, class-agnostic target detection, NMS, and Picture-in-Picture (PiP) zoom thumbnail overlay.
- **Types & Enums**:
  ```rust
  pub struct YoloDetector { ... }
  ```
- **Public Functions & Signatures**:
  ```rust
  impl YoloDetector {
      pub fn new(model_path: &Path) -> Result<Self>;
      pub fn input_size(&self) -> u32;
      pub fn detect(&mut self, image: &DynamicImage, threshold: f32, targets: &[String]) -> Result<Vec<Detection>>;
      pub fn annotate_frame(image: &DynamicImage, detections: &[Detection]) -> RgbImage;
  }
  ```

### `src/api/telegram.rs` (Role: api, Lines: 393)
- **Responsibility**: Two-way Telegram bot integration with interactive inline button keyboards, toast callback queries, command polling, multipart photo/animation alerts, and rate-limited FIFO bounded queue (1 msg/sec pacing with HTTP 429 backoff & retry).
- **Public Functions & Signatures**:
  ```rust
  pub enum BotCommand { Status, Arm, Disarm, Mute(u64), Snap, Help }
  pub enum OutgoingMessage {
      Text(String),
      Photo { bytes: Vec<u8>, caption: String },
      Animation { bytes: Vec<u8>, caption: String },  // NEW
  }
  impl TelegramClient {
      pub fn new(token: Option<String>, chat_id: Option<String>) -> Self;
      pub fn is_configured(&self) -> bool;
      pub fn send_message(&self, text: &str) -> Result<bool>;
      pub fn send_photo_alert(&self, image_bytes: Vec<u8>, caption: &str) -> Result<bool>;
      pub fn send_animation_alert(&self, gif_bytes: Vec<u8>, caption: &str) -> Result<bool>;  // NEW
      pub fn answer_callback(&self, query_id: &str, toast: &str) -> Result<()>;
      pub fn poll_commands(&mut self) -> Result<Vec<BotCommand>>;
  }
  ```

### `src/sentry.rs` (Role: sentry, Lines: 397)
- **Responsibility**: Core guardian loop coordinating stream reconnect (exponential backoff), motion-masked detection, 2-frame temporal confirmation, GIF/JPEG dispatch, periodic evidence pruning, and Telegram command/button handling.
- **Constants**:
  ```rust
  const FRAME_BUFFER_CAP: usize = 30;        // rolling ring-buffer size
  const GIF_FRAME_DELAY_MS: u32 = 100;       // 10fps animated GIF
  const CONFIRM_WINDOW: Duration = 700ms;    // 2-frame confirmation window
  const MAX_BACKOFF_SECS: u64 = 30;          // reconnect ceiling
  const PRUNE_INTERVAL: Duration = 6h;       // housekeeping cadence
  ```
- **Public Functions & Signatures**:
  ```rust
  impl RoomSentry {
      pub fn new(config: SentryConfig) -> Result<Self>;
      pub fn process_frame(&mut self, image: &DynamicImage) -> Result<Vec<Detection>>;
      pub fn run_test(&mut self) -> Result<()>;
      pub fn run_loop(&mut self) -> Result<()>;
  }
  ```

### `src/sentry/replies.rs` (Role: sentry, Lines: 165)
- **Responsibility**: Bot reply helpers extracted from sentry.rs to keep it under 400 lines. Handles `/status`, `/help`, `/snap` output and mute_alerts state update.
- **Public Functions & Signatures**:
  ```rust
  pub fn mute_alerts(telegram: &TelegramClient, mute_until: &mut Option<Instant>, mins: u64) -> Result<()>;
  pub fn send_help(telegram: &TelegramClient) -> Result<()>;
  pub fn send_status(telegram: &TelegramClient, config: &SentryConfig, armed: bool, mute_until: Option<Instant>, last_alert: Option<Instant>, start_time: Instant) -> Result<()>;
  pub fn send_snapshot(telegram: &TelegramClient, detector: &mut YoloDetector, config: &SentryConfig, frame: &DynamicImage) -> Result<()>;
  pub fn ist_now_str() -> String;  // shared IST timestamp formatter
  ```

## 4. Execution Lifecycle Trace
1. **Startup**: `src/main.rs` parses CLI args. If `--setup`, runs interactive terminal wizard and exits.
2. **Dynamic Dylib**: Resolves `libonnxruntime.so` to absolute canonical path and calls `ort::init_from`.
3. **Config Discovery**: Priority: CLI > `~/.config/monban/config.toml` > `~/.config/tayori/config.toml`.
4. **Startup Housekeeping**: `prune_old_evidence` deletes stale `sentry_*.jpg` captures older than `retention_days` (default 7).
5. **Guard Loop (Auto-Reconnect)**: Outer `'outer` loop retries `MjpegStream::connect` with exponential backoff (2→4→8→30s) on failure; Telegram offline/online alerts sent.
6. **Per-Frame Pipeline**: Reads latest frame → pushes to 30-frame ring buffer → `check_motion` (skips YOLO if static) → `detect` → `apply_motion_mask` (bbox must overlap motion pixels) → `temporal_confirm` (2-frame window 600ms) → `handle_alert`.
7. **Alert Dispatch**: If ≥4 frames in buffer: encodes animated GIF (320×240 @ 10fps) via `encode_animated_gif` + `sendAnimation`. Falls back to static JPEG `send_photo_alert` on encode failure.
8. **Two-Way Control**: Sentry polls authorized Telegram commands every 1.5s (`/status`, `/snap`, `/arm`, `/disarm`, `/mute`, `/help`).
9. **Periodic Housekeeping**: `prune_old_evidence` runs every 6 hours during the guard loop.

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
- **2026-09-26 (v0.4.0)**: **6 Guardian Upgrades** — Motion-masked bounding boxes (`has_motion_in_box`), 2-frame temporal confirmation (600ms window), animated GIF motion clips (30-frame ring buffer, 320×240 @ 10fps via `sendAnimation`), ROI top-exclusion (`--ignore-top`, configurable per-frame %), evidence disk retention (`--retention-days`, default 7, pruned on startup + every 6h), camera auto-reconnect with Telegram offline/online alerts (exponential backoff 2→30s).
- **2026-09-26 (v0.3.6)**: Class-agnostic detection & clean alert captions: removed confusing COCO class names from Telegram notifications and manual snapshot replies; detector now extracts peak confidence across candidate anchors and assigns clean, generic `"target"` labeling with confidence percentage.
- **2026-09-26 (v0.3.5)**: Rate-limited FIFO message queue & automatic HTTP 429 retry: implemented internal bounded worker channel (`sync_channel(10)`) and background pacing dispatcher enforcing Telegram's 1.0s inter-message limit; added exponential backoff on HTTP 429 / network errors reading `Retry-After`; eliminated ad-hoc thread spawning in sentry loop.
- **2026-09-26 (v0.3.4)**: Real-time stream latency & non-blocking alert refactor: re-architected `MjpegStream` to zero-lag drop-stale raw JPEG storage (instantly discards older backlog frames, eliminating 2–4s queue lag when moving camera); made Telegram photo alert uploads non-blocking via detached background threads, keeping sentry video pipeline 100% fluid at real-time speeds.
- **2026-09-26 (v0.3.3)**: Purged `cat` and irrelevant classes from default targets, added `--rotate <DEGREES>` stream rotation support.
- **2026-09-26 (v0.3.2)**: Target class whitelisting & confidence calibration.
- **2026-09-26 (v0.3.1)**: Added adaptive model input resolution detection from ONNX tensor graph.
- **2026-09-26 (v0.3.0)**: Added multi-object classification across all 80 COCO classes, PiP zoom thumbnail overlay, Telegram inline buttons.
- **2026-09-26**: Added ultra-low-overhead pixel difference `MotionDetector` gating. Two-way Telegram bot command control. IST log timestamps. ONNX dylib loader fix. Complete Rust port.
