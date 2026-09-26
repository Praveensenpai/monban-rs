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
         FrameBuffer (VecDeque<DynamicImage>, cap=30)  <── Rolling ring-buffer for Video/GIF
                  │
                  ▼
         MotionDetector (128x96 grayscale, ignore_top_percent, adaptive EMA noise)
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
         apply_watch_rect() -> normalized inclusion zone filtering
                  │
                  ▼
         AlertSession.extend() & SentryStats.record_session()
                  │
                  ▼
         temporal_confirm() -> 2-frame window (700ms)
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
                                fire_webhook() (Home Assistant JSON)
                                       │
                                       ▼
                        Media Dispatch Cascade
                                       │
                    ┌──────────────────┼──────────────────┐
                    ▼                  ▼                  ▼
            encode_video_clip()   encode_animated_gif()   Static JPEG
              (FFmpeg H.264)         (Image GIF)           (Fallback)
                    │                  │                  │
                    ▼                  ▼                  ▼
                sendVideo        sendAnimation       sendPhoto
```

## 2. Global Constraints & Architecture Patterns
- **Primary Language & Edition**: Rust 2024 edition (Rust 1.97.1).
- **Architectural Paradigm**: Role-based (`domain/`, `infra/`, `api/`, `cli/`, `sentry`).
- **Hard Constraints**: <400 lines/file, <60 lines/fn, max 4 parameters, zero production `unwrap()`/`expect()`, zero compiler/clippy warnings (`-D warnings`).
- **Target Distribution**: Standalone Linux x86_64 binary (`11 MB`), **Peak RSS: ~69 MB**.

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

### `src/domain/models.rs` (Role: domain, Lines: 153)
- **Responsibility**: Pure domain models for bounding boxes, area calculation, Intersection-over-Union (IoU), 80 COCO classes, emoji mapping, and target guardian class filtering.
- **Types & Enums**:
  ```rust
  pub struct BoundingBox { pub x1: f32, pub y1: f32, pub x2: f32, pub y2: f32 }
  pub struct Detection { pub class_id: usize, pub label: String, pub confidence: f32, pub box_coords: BoundingBox }
  pub const COCO_CLASSES: [&str; 80] = [ ... ];
  pub const DEFAULT_TARGET_CLASSES: [&str; 3] = [ "person", "dog", "cow" ];
  ```

### `src/domain/stats.rs` (Role: domain, Lines: 104)
- **Responsibility**: In-memory sentry alert statistics, frame counting, session counting, and 24-hour alert distribution histogram with ASCII bar rendering for `/stats` Telegram commands.
- **Types & Enums**:
  ```rust
  pub struct SentryStats {
      pub total_alerts: u64,
      pub session_count: u64,
      pub frames_processed: u64,
      hourly_histogram: [u32; 24],
      started_at: Instant,
  }
  impl SentryStats {
      pub fn new() -> Self;
      pub fn record_alert(&mut self);
      pub fn record_session(&mut self);
      pub fn record_frame(&mut self);
      pub fn format_message(&self) -> String;
  }
  ```

### `src/domain/config.rs` (Role: domain, Lines: 243)
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
      pub ignore_top_percent: u32,
      pub retention_days: u32,
      pub watch_rect: Option<[f32; 4]>,
      pub webhook_url: String,
      pub heartbeat_hours: u64,
      pub adaptive_motion: bool,
      pub extra_sources: Vec<String>,
  }
  ```

### `src/infra/motion.rs` (Role: infra, Lines: 141)
- **Responsibility**: Fast grayscale pixel-difference motion detector with 128x96 grid, ROI top exclusion, per-pixel motion mask, and EMA-based adaptive noise threshold auto-tuning.
- **Public Functions & Signatures**:
  ```rust
  impl MotionDetector {
      pub fn new(threshold: f32) -> Self;
      pub fn with_ignore_top(self, percent: u32) -> Self;
      pub fn with_adaptive(self, enabled: bool) -> Self;
      pub fn check_motion(&mut self, image: &DynamicImage) -> bool;
      pub fn has_motion_in_box(&self, bbox: &BoundingBox, orig_w: f32, orig_h: f32) -> bool;
      pub fn reset(&mut self);
  }
  ```

### `src/infra/video.rs` (Role: infra, Lines: 66)
- **Responsibility**: Subprocess FFmpeg H.264 video encoder (`encode_video_clip`) generating lightweight MP4 motion clips (320x240 @ 10fps, CRF 30, ultrafast preset).

### `src/infra/webhook.rs` (Role: infra, Lines: 34)
- **Responsibility**: Non-blocking, non-fatal Home Assistant / generic HTTP JSON webhook dispatcher (`fire_webhook`).

### `src/infra/gif.rs` (Role: infra, Lines: 34)
- **Responsibility**: GIF encoder (`encode_animated_gif`) using image crate as fallback when FFmpeg is unavailable.

### `src/infra/storage.rs` (Role: infra, Lines: 59)
- **Responsibility**: Automatic evidence retention housekeeping (`prune_old_evidence`).

### `src/api/telegram/types.rs` (Role: api, Lines: 68)
- **Responsibility**: Data types for Telegram bot commands, media payloads, and webhook responses.
- **Types & Enums**:
  ```rust
  pub enum BotCommand { Status, Stats, Arm, Disarm, Mute(u64), Snap, Help }
  pub enum OutgoingMessage { Text(String), Photo { ... }, Animation { ... }, Video { ... } }
  pub struct MediaPayload<'a> { ... }
  ```

### `src/api/telegram.rs` (Role: api, Lines: 377)
- **Responsibility**: Telegram Bot client with bounded queue, rate-limited worker thread, media dispatch, callback queries, and command polling.

### `src/sentry/session.rs` (Role: sentry, Lines: 60)
- **Responsibility**: Continuous presence alert session lifecycle tracking (`AlertSession`), duration computation, and session summary message generation.

### `src/sentry/dispatch.rs` (Role: sentry, Lines: 95)
- **Responsibility**: Frame filtering (`apply_motion_mask`, `apply_watch_rect`), detection formatting, and media dispatch cascade (MP4 -> GIF -> JPEG).

### `src/sentry/replies.rs` (Role: sentry, Lines: 162)
- **Responsibility**: Bot response builders for `/status`, `/stats`, `/snap`, `/mute`, and `/help`.

### `src/sentry/runner.rs` (Role: sentry, Lines: 96)
- **Responsibility**: Sentry run loops (`run_test` and `run_loop`) with auto-reconnection and exponential backoff.

### `src/sentry.rs` (Role: sentry, Lines: 313)
- **Responsibility**: Core sentry coordinator wiring stream frames, motion gating, YOLO inference, session tracking, statistics, heartbeat, and housekeeping.

## 4. Execution Lifecycle Trace
1. **Startup**: `src/main.rs` parses CLI args. If `--setup`, runs interactive terminal wizard and exits.
2. **Dynamic Dylib**: Resolves `libonnxruntime.so` to absolute canonical path and calls `ort::init_from`.
3. **Multi-Camera**: Spawns background sentry threads for each `--extra-sources` camera URL.
4. **Housekeeping & Retention**: `prune_old_evidence` removes expired snapshots on startup and every 6h.
5. **Guard Loop**: Every cycle reads latest frame -> pushes to 30-frame ring buffer -> adaptive motion gating -> YOLO detection -> motion mask filter -> inclusion zone filter -> session update -> temporal confirmation -> alert media cascade (MP4 -> GIF -> JPEG) -> Home Assistant webhook -> Telegram dispatch.
6. **Heartbeat & Sessions**: Periodically sends "still alive" heartbeat and session summary on intruder departure.

## 5. Verification Commands
```bash
# Lint, format, and typecheck
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# Automated test suite (14 unit/integration tests)
cargo test --all-targets

# Live test with camera
monban --test
```

## 6. Recent Iteration Changes
- **2026-09-26 (v0.5.0)**: **Full Guardian Intelligence & Automation Suite**:
  - **MP4 Video Clips**: H.264 video encoding via `src/infra/video.rs` (`encode_video_clip`) with automatic fallback to GIF and JPEG.
  - **Daily Heartbeat**: Periodic Telegram status pulse (`--heartbeat-hours`, default 24) reporting uptime, total alerts, and active monitoring state.
  - **Adaptive Motion Gating**: Real-time noise EMA tracking auto-adjusts threshold on breezy/noisy backgrounds (`--adaptive-motion`).
  - **Inclusion Zones**: Normalized ROI watch zones (`--watch-rect x1,y1,x2,y2`) restricting alerts to critical areas (e.g. doors).
  - **Alert Sessions & Presence Tracking**: `AlertSession` groups continuous sightings into single events and emits departure summaries.
  - **`/stats` Telegram Command**: In-memory 24h alert distribution with ASCII histogram bar charts and interactive inline button.
  - **Multi-Camera Support**: Seamless multi-stream processing via `--extra-sources` running concurrent sentry threads.
  - **Home Assistant Webhook**: Instant JSON webhook dispatch (`--webhook-url`) on intruder detections for home automation triggers.
  - **Strict Architecture Refactoring**: Extracted `sentry/dispatch.rs`, `sentry/runner.rs`, `sentry/session.rs`, `domain/stats.rs`, `infra/video.rs`, `infra/webhook.rs`, and `api/telegram/types.rs` keeping all files <380 lines.
