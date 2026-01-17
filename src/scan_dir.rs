use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, Duration};

pub fn scan_dir(path_str: String, tx: Sender<String>) {
    std::thread::spawn(move || {
        let target_path = if path_str.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(&path_str)
        };

        let now = SystemTime::now();
        let format_ago = |time: SystemTime| {
            match now.duration_since(time) {
                Ok(dur) => {
                    let secs = dur.as_secs();
                    if secs < 60 { "Just now".into() }
                    else if secs < 3600 { format!("{}m ago", secs / 60) }
                    else if secs < 86400 { format!("{}h ago", secs / 3600) }
                    else { format!("{}d ago", secs / 86400) }
                }
                Err(_) => "Future".into(),
            }
        };

        loop {
            if let Ok(entries) = fs::read_dir(&target_path) {
                // Signal start of new scan
                if tx.send("__CLEAR__".to_string()).is_err() { break; }

                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();

                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        let file_type = meta.file_type();
                        let prefix = if file_type.is_symlink() { "[LNK]" } 
                                    else if file_type.is_dir() { "[DIR]" } 
                                    else { "[FILE]" };

                        let s = meta.len();
                        let s_str = if file_type.is_dir() { "---".into() }
                                    else if s >= 1_048_576 { format!("{:.2} MB", s as f64 / 1_048_576.0) }
                                    else if s >= 1_024 { format!("{:.2} KB", s as f64 / 1_024.0) }
                                    else { format!("{} B", s) };

                        let mtime = meta.modified().map(format_ago).unwrap_or_else(|_| "N/A".into());
                        let atime = meta.accessed().map(format_ago).unwrap_or_else(|_| "N/A".into());

                        let data = format!("{};{};{};{};{}", prefix, name, s_str, mtime, atime);
                        if tx.send(data).is_err() { return; }
                    }
                }
                // SIGNAL DONE: The UI will only swap buffers now
                if tx.send("__DONE__".to_string()).is_err() { break; }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}