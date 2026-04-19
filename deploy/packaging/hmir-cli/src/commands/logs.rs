use std::{fs, path::PathBuf, thread, time::Duration};

pub fn run_logs(tail: usize, grep: Option<&str>, follow: bool, show_dir: bool) {
    let dir = log_dir();

    if show_dir {
        println!("{}", dir.display());
        return;
    }

    if follow {
        follow_logs(tail, grep);
        return;
    }

    println!("{}", filtered_snapshot(tail, grep));
}

fn follow_logs(tail: usize, grep: Option<&str>) {
    println!("Following logs. Press Ctrl+C to stop.");
    let mut last = String::new();

    loop {
        let current = filtered_snapshot(tail, grep);
        if current != last {
            if !last.is_empty() {
                println!();
                println!("--- log update ---");
            }
            print!("{}", current);
            last = current;
        }
        thread::sleep(Duration::from_millis(1200));
    }
}

fn filtered_snapshot(tail: usize, grep: Option<&str>) -> String {
    let mut sections = Vec::new();

    for name in ["api.log", "dashboard_error.log"] {
        let path = log_dir().join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            let filtered = tail_lines(&filter_lines(&content, grep), tail);
            if !filtered.trim().is_empty() {
                sections.push(format!("===== {} =====\n{}\n", name, filtered));
            }
        }
    }

    if sections.is_empty() {
        return format!(
            "No HMIR logs found yet in {}.\n",
            log_dir().display()
        );
    }

    sections.join("\n")
}

fn filter_lines(content: &str, grep: Option<&str>) -> String {
    let Some(needle) = grep.map(|value| value.to_lowercase()) else {
        return content.to_string();
    };

    content
        .lines()
        .filter(|line| line.to_lowercase().contains(&needle))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tail_lines(content: &str, tail: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(tail);
    lines[start..].join("\n")
}

fn log_dir() -> PathBuf {
    data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hmir")
        .join("logs")
}

fn data_local_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local").join("share"))
            })
    }
}
