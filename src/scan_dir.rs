use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, Duration};

#[derive(Clone, Copy, PartialEq)]
pub enum SortProperty {
    Name,
    Size,
    Modified,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

pub enum ScanMsg {
    Clear,
    Done,
    Entry(FileDisplay),
}

struct FileEntry {
    is_dir: bool,
    name: String,
    size: u64,
    mtime: SystemTime,
    display_data: FileDisplay,
}

pub struct FileDisplay {
    pub prefix: String,
    pub name: String,
    pub size: String,
    pub mtime: String,
    pub atime: String,
    pub ctime: String,
}

pub fn scan_dir(
    path_str: String, 
    tx: Sender<ScanMsg>, 
    prop: SortProperty, 
    order: SortOrder, 
    show_hidden: bool // New triggerable flag
) {
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
                let mut file_entries = Vec::new();

                for entry in entries.flatten() {
                    let raw_name = entry.file_name().to_string_lossy().into_owned();

                    // --- TRIGGERABLE FILTER ---
                    if !show_hidden && raw_name.starts_with('.') {
                        continue;
                    }

                    let path = entry.path();
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        let file_type = meta.file_type();
                        let is_dir = file_type.is_dir();
                        let prefix = if file_type.is_symlink() { "[LINK]" } 
                                    else if is_dir { "[DIR]" } 
                                    else { "[FILE]" };

                        let s = meta.len();
                        let s_str = if is_dir { "---".into() }
                                    else if s >= 1_048_576 { format!("{:.2} MB", s as f64 / 1_048_576.0) }
                                    else if s >= 1_024 { format!("{:.2} KB", s as f64 / 1_024.0) }
                                    else { format!("{} B", s) };

                        let mtime_raw = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                        
                        // 3. Create the struct here instead of formatting a string
                        let display = FileDisplay {
                            prefix: prefix.to_string(),
                            name: raw_name.clone(),
                            size: s_str,
                            mtime: format_ago(mtime_raw),
                            atime: meta.accessed().map(format_ago).unwrap_or_else(|_| "N/A".into()),
                            ctime: meta.created().map(format_ago).unwrap_or_else(|_| "N/A".into()),
                        };
                        
                        file_entries.push(FileEntry {
                            is_dir,
                            name: raw_name.to_lowercase(),
                            size: s,
                            mtime: mtime_raw,
                            display_data: display,
                        });
                    }
                }

                // --- ADVANCED SORTING ENGINE ---
                file_entries.sort_by(|a, b| {
                    // 1. Primary: Directories always first
                    if a.is_dir != b.is_dir {
                        return b.is_dir.cmp(&a.is_dir);
                    }

                    // 2. Secondary: Chosen Property
                    let mut cmp = match prop {
                        SortProperty::Name => a.name.cmp(&b.name),
                        SortProperty::Size => a.size.cmp(&b.size),
                        SortProperty::Modified => a.mtime.cmp(&b.mtime),
                    };

                    // Apply Order (Asc/Desc) to the property comparison
                    if order == SortOrder::Desc {
                        cmp = cmp.reverse();
                    }

                    // 3. Tertiary: Fallback to Alphabetical if property is equal (e.g., same size)
                    if cmp == std::cmp::Ordering::Equal {
                        a.name.cmp(&b.name)
                    } else {
                        cmp
                    }
                });

                // Send the structured messages
                if tx.send(ScanMsg::Clear).is_ok() {
                    for entry in file_entries {
                        // Move the struct to the UI thread (Zero-copy)
                        if tx.send(ScanMsg::Entry(entry.display_data)).is_err() { 
                            return; 
                        }
                    }
                    if tx.send(ScanMsg::Done).is_err() {
                        return; 
                    }
                } else {
                    return; // KILL THE THREAD
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}