use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Scans directory and prunes capture files older than `retention_days`.
pub fn prune_old_evidence(save_dir: &Path, retention_days: u32) -> usize {
    if retention_days == 0 || !save_dir.exists() {
        return 0;
    }

    let Ok(entries) = fs::read_dir(save_dir) else {
        return 0;
    };

    let max_age = Duration::from_secs(retention_days as u64 * 86_400);
    let now = SystemTime::now();
    let mut pruned_count = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("sentry_") {
            continue;
        }

        let Ok(meta) = entry.metadata() else {
            continue;
        };

        let Ok(modified) = meta.modified() else {
            continue;
        };

        if let Ok(age) = now.duration_since(modified)
            && age > max_age
        {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to prune old evidence file {:?}: {e}", path);
            } else {
                debug!("Pruned old capture: {:?}", path);
                pruned_count += 1;
            }
        }
    }

    if pruned_count > 0 {
        info!(
            "Storage housekeeping: pruned {} capture(s) older than {} day(s)",
            pruned_count, retention_days
        );
    }

    pruned_count
}
