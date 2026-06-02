use std::fs;
use std::process::Command;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    pub exec: String, // The actual binary/command to run
}

pub fn get_installed_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    let path = "/usr/share/applications";
    
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                if let Ok(content) = fs::read_to_string(path) {
                    let mut name = None;
                    let mut exec = None;
                    for line in content.lines() {
                        if line.starts_with("Name=") && name.is_none() {
                            name = Some(line[5..].to_string());
                        }
                        if line.starts_with("Exec=") && exec.is_none() {
                            // Extract command, removing %f, %u, etc.
                            let cmd = line[5..]
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .replace("%f", "")
                                .replace("%u", "")
                                .to_string();
                            exec = Some(cmd);
                        }
                    }
                    if let (Some(n), Some(e)) = (name, exec) {
                        // Only add if the executable isn't empty
                        if !e.is_empty() {
                            apps.push(AppEntry { name: n, exec: e });
                        }
                    }
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

pub fn launch_app(cmd: &str, file_path: PathBuf) {
    // DO NOT use xdg-open. Use the binary command directly.
    // Example: If cmd is "vlc", this runs `vlc /path/to/file`
    let _ = Command::new(cmd).arg(file_path).spawn();
}
