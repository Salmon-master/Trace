use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILENAME;

/// Find a hardware configuration in `start` or one of its parents.
pub fn find_config(start: impl AsRef<Path>) -> Option<PathBuf> {
    let mut directory = start.as_ref().to_path_buf();

    loop {
        let candidate = directory.join(CONFIG_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }

        if !directory.pop() {
            return None;
        }
    }
}
