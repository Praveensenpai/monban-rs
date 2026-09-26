# 🥋 門番 (Monban-rs) — High-Performance Rust AI Room Sentry

[![Rust 2021](https://img.shields.io/badge/Rust-2021-DEA584?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Binary Size: 11MB](https://img.shields.io/badge/Binary%20Size-11MB-blue?style=flat-square)](target/release/monban-rs)
[![RAM Footprint: ~69MB](https://img.shields.io/badge/RAM%20Footprint-69MB-success?style=flat-square)](#-performance-benchmarks)
[![Inference: 42ms](https://img.shields.io/badge/Inference-42ms-brightgreen?style=flat-square)](#-performance-benchmarks)
[![Clippy: 0 warnings](https://img.shields.io/badge/Clippy-0%20warnings-brightgreen?style=flat-square)](https://github.com/rust-lang/rust-clippy)

> **Blazingly fast, zero-bloat AI room & door guardian written in pure Rust.** Uses hardware-accelerated ONNX Runtime YOLOv8-nano to detect humans with zero buffering lag, saving photographic evidence and delivering instant photo alerts to your Telegram.

---

## ⚡ Performance Benchmarks

| Metric | Python Monban (PyTorch + OpenCV) | **Rust Monban-rs (ORT + Pure Rust)** | Efficiency Gain |
| :--- | :--- | :--- | :--- |
| **RAM Footprint (RSS)** | **818 MB** | **69 MB** | 🚀 **91.5% Less RAM** |
| **Inference Time / Frame** | **~560 ms** | **42 ms** | ⚡ **13.3x Faster** |
| **Disk Size** | **~2.2 GB** (`.venv` + CUDA/wheels) | **11 MB** (Single executable) | 📦 **99.5% Smaller** |
| **Stream Latency** | Buffer queue lag | **Zero-lag real-time** | 🎯 **Instant** |

---

## 🏛️ Architecture

```text
  ┌────────────────────────────────────────────────────────┐
  │                   VIDEO SOURCE                         │
  │     Phone Wi-Fi Stream (DroidCam / HTTP MJPEG)         │
  └───────────────────────────┬────────────────────────────┘
                              │ HTTP Multipart Stream
                              ▼
  ┌────────────────────────────────────────────────────────┐
  │                 PURE-RUST MJPEG WORKER                 │
  │            Scans 0xFF,0xD8 .. 0xFF,0xD9 Byte Slices    │
  └───────────────────────────┬────────────────────────────┘
                              │ Latest Frame (DynamicImage)
                              ▼
  ┌────────────────────────────────────────────────────────┐
  │                 YOLOv8n ONNX ENGINE                    │
  │        Hardware Vectorized (AVX2/SIMD) in 42ms         │
  └───────────────────────────┬────────────────────────────┘
                              │ Person Found?
                              ▼
  ┌────────────────────────────────────────────────────────┐
  │                  COOLDOWN FILTER                       │
  │           Prevents Spam (Configurable Window)          │
  └───────────────────────────┬────────────────────────────┘
                              │ Outside Cooldown
                              ▼
         ┌────────────────────┴────────────────────┐
         ▼                                         ▼
┌──────────────────┐                     ┌──────────────────┐
│  Disk Evidence   │                     │  Telegram Alert  │
│ captures/*.jpg   │                     │  Instant Photo   │
└──────────────────┘                     └──────────────────┘
```

---

## 🚀 Quickstart

### 1. Build
```bash
cargo build --release
```

### 2. Single-Frame Test
```bash
./target/release/monban-rs --test --source http://192.168.1.36:4747/video
```

### 3. Continuous Background Guardian
```bash
./target/release/monban-rs --source http://192.168.1.36:4747/video
```

---

## ⚙️ Options

```text
Options:
  -s, --source <SOURCE>
          Video stream URL (http://...) [default: http://192.168.1.36:4747/video]
  -m, --model <MODEL>
          Path to YOLOv8 ONNX model (defaults to yolov8n.onnx or ~/.local/share/monban/yolov8n.onnx)
  -c, --confidence <CONFIDENCE>
          Detection confidence threshold (0.0 - 1.0) [default: 0.45]
      --targets <TARGETS>
          Comma-separated target classes to detect (default: person,dog,cow)
      --rotate <ROTATE>
          Rotate video feed clockwise in degrees (0, 90, 180, 270) [default: 0]
      --cooldown <COOLDOWN>
          Seconds between Telegram alerts [default: 5]
      --save-dir <SAVE_DIR>
          Directory to save snapshot evidence [default: captures]
      --test
          Test mode: capture 1 frame, check detection, and exit
      --setup
          Run interactive setup wizard to configure dedicated Telegram bot
      --motion-threshold <MOTION_THRESHOLD>
          Pixel difference threshold ratio for motion detection gating (0.0005 - 1.0) [default: 0.005]
      --no-motion-gate
          Disable motion gating and run YOLO inference on every frame
  -v, --verbose
          Enable verbose debug logging
  -h, --help
          Print help
  -V, --version
          Print version
```
