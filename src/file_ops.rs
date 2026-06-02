use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::fs;
use crate::scan_dir::FileDisplay;

// Helper function local to this module
fn get_current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn cut_item(selected_indices: &HashSet<usize>, file_list: &[FileDisplay]) -> Vec<PathBuf> {
    selected_indices.iter()
        .map(|&idx| get_current_dir().join(&file_list[idx].name))
        .collect()
}

pub fn copy_item(selected_indices: &HashSet<usize>, file_list: &[FileDisplay]) -> Vec<PathBuf> {
    selected_indices.iter()
        .map(|&idx| get_current_dir().join(&file_list[idx].name))
        .collect()
}

pub fn paste_item(clipboard: &[PathBuf], dest: &PathBuf) -> std::io::Result<()> {
    for path in clipboard {
        if let Some(name) = path.file_name() {
            let dest_path = dest.join(name);
            if path.is_dir() {
                // Call copy_dir_recursive within the same module
                copy_dir_recursive(path, &dest_path)?;
            } else {
                fs::copy(path, dest_path)?;
            }
        }
    }
    Ok(())
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), &dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
