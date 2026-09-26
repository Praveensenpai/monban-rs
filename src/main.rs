use chrono::{FixedOffset, Utc};
use clap::Parser;
use monban_rs::cli::CliArgs;
use monban_rs::domain::SentryConfig;
use monban_rs::error::Result;
use monban_rs::sentry::RoomSentry;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

struct IstTime;

impl FormatTime for IstTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let ist_offset = FixedOffset::east_opt(19800);
        let formatted = match ist_offset {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format("%Y-%m-%d %H:%M:%S IST"),
            None => Utc::now()
                .with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S UTC"),
        };
        write!(w, "{formatted}")
    }
}

fn setup_logging(verbose: bool) {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .with_timer(IstTime)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn ensure_onnx_dylib() {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        let p = std::path::PathBuf::from(path);
        if let Ok(abs) = std::fs::canonicalize(&p) {
            let _ = ort::init_from(&abs).map(|b| b.commit());
        }
        return;
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let user_local_lib = std::path::PathBuf::from(format!("{home}/.local/lib/libonnxruntime.so"));

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(parent) = std::env::current_exe()
        .ok()
        .as_ref()
        .and_then(|p| p.parent())
    {
        candidates.push(parent.join("libonnxruntime.so"));
        candidates.push(parent.join("../lib/libonnxruntime.so"));
    }
    candidates.push(user_local_lib);
    candidates.push(std::path::PathBuf::from("/usr/local/lib/libonnxruntime.so"));
    candidates.push(std::path::PathBuf::from("/usr/lib/libonnxruntime.so"));
    candidates.push(std::path::PathBuf::from("libonnxruntime.so"));
    candidates.push(std::path::PathBuf::from("target/release/libonnxruntime.so"));

    for candidate in &candidates {
        if let Ok(abs_path) = std::fs::canonicalize(candidate) {
            // Safe: called at the start of main before any worker threads are spawned
            unsafe {
                std::env::set_var("ORT_DYLIB_PATH", &abs_path);
            }
            let _ = ort::init_from(&abs_path).map(|b| b.commit());
            return;
        }
    }
}

fn main() -> Result<()> {
    let args = CliArgs::parse();
    setup_logging(args.verbose);

    if args.setup {
        monban_rs::infra::run_interactive_setup()?;
        return Ok(());
    }

    ensure_onnx_dylib();

    let motion_gate = if args.no_motion_gate {
        Some(false)
    } else {
        None
    };

    let config = SentryConfig::load_with_defaults(
        Some(args.source),
        args.model,
        Some(args.confidence),
        Some(args.cooldown),
        Some(args.save_dir),
        motion_gate,
        Some(args.motion_threshold),
    );

    let mut sentry = RoomSentry::new(config)?;

    if args.test {
        sentry.run_test()?;
    } else {
        sentry.run_loop()?;
    }

    Ok(())
}
