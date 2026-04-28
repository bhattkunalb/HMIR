use std::path::PathBuf;
use std::fs;

pub async fn run_clean() {
    println!("🧹 HMIR | COMMENCING RUNTIME CLEANUP");
    
    // 1. Resolve cache directory
    // cspell:ignore USERPROFILE
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
    let hmir_models_dir = std::path::Path::new(&home).join(".hmir").join("models");
    let local_app_data_hmir = std::env::var("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("hmir").join("models"))
        .unwrap_or_else(|_| PathBuf::from("."));

    let target_dirs = vec![hmir_models_dir, local_app_data_hmir];
    let mut purged_count = 0;

    for base_dir in target_dirs {
        if !base_dir.exists() { continue; }
        
        println!("🔍 Scanning: {}", base_dir.display());
        
        // Scan subdirectories for cache folders
        if let Ok(entries) = fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check for common cache folder names
                    for cache_name in ["cache", "cl_cache", "blob_cache"] {
                        let potential_cache = path.join(cache_name);
                        if potential_cache.exists() && potential_cache.is_dir() {
                            println!("  🗑 Purging: {}", potential_cache.display());
                            if let Err(e) = fs::remove_dir_all(&potential_cache) {
                                println!("  ⚠️ Failed to purge {}: {}", potential_cache.display(), e);
                            } else {
                                purged_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    if purged_count > 0 {
        println!("\n✅ Successfully purged {} cache locations.", purged_count);
        println!("✨ Model loading should now be performant and error-free.");
    } else {
        println!("\n✅ No stale caches found. System is clean.");
    }
}
