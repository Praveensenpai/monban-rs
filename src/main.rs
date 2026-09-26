use clap::Parser;
use monban_rs::cli::CliArgs;
use monban_rs::domain::SentryConfig;
use monban_rs::error::Result;
use monban_rs::sentry::RoomSentry;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn setup_logging(verbose: bool) {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn ensure_onnx_dylib() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        return;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let user_local_lib = format!("{home}/.local/lib/libonnxruntime.so");
    for candidate in &[
        "libonnxruntime.so",
        "target/release/libonnxruntime.so",
        &user_local_lib,
        "/usr/local/lib/libonnxruntime.so",
    ] {
        let p = std::path::Path::new(candidate);
        if p.exists() {
            // Safe: called at the start of main before any threads are spawned
            unsafe {
                std::env::set_var("ORT_DYLIB_PATH", p);
            }
            return;
        }
    }
}

fn main() -> Result<()> {
    ensure_onnx_dylib();
    let args = CliArgs::parse();
    setup_logging(args.verbose);

    let config = SentryConfig::load_with_defaults(
        Some(args.source),
        Some(args.model),
        Some(args.confidence),
        Some(args.cooldown),
        Some(args.save_dir),
    );

    let mut sentry = RoomSentry::new(config)?;

    if args.test {
        sentry.run_test()?;
    } else {
        sentry.run_loop()?;
    }

    Ok(())
}
