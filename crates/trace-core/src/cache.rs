use directories::ProjectDirs;
use std::path::PathBuf;

/// Location for disposable, machine-local Trace data.
pub fn cache_directory() -> Option<PathBuf> {
    ProjectDirs::from("", "Trace", "trace").map(|dirs| dirs.cache_dir().to_path_buf())
}
