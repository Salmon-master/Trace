use directories::ProjectDirs;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use thiserror::Error;

use crate::ConnectivityGraph;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("could not determine the Trace cache directory")]
    DirectoryUnavailable,
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to decode cached graph {path}: {source}")]
    Decode {
        path: String,
        source: serde_json::Error,
    },
    #[error("failed to encode cached graph: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Location for disposable, machine-local Trace data.
pub fn cache_directory() -> Option<PathBuf> {
    ProjectDirs::from("", "Trace", "trace").map(|dirs| dirs.cache_dir().to_path_buf())
}

pub fn schematic_hash(path: impl AsRef<std::path::Path>) -> Result<String, CacheError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| CacheError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn graph_path(schematic: impl AsRef<std::path::Path>) -> Result<PathBuf, CacheError> {
    let root = cache_directory().ok_or(CacheError::DirectoryUnavailable)?;
    Ok(root
        .join("projects")
        .join(schematic_hash(schematic)?)
        .join("graph.json"))
}

pub fn load_graph(path: impl AsRef<std::path::Path>) -> Result<ConnectivityGraph, CacheError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| CacheError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CacheError::Decode {
        path: path.display().to_string(),
        source,
    })
}

pub fn store_graph(
    path: impl AsRef<std::path::Path>,
    graph: &ConnectivityGraph,
) -> Result<(), CacheError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CacheError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let bytes = serde_json::to_vec_pretty(graph)?;
    std::fs::write(path, bytes).map_err(|source| CacheError::Write {
        path: path.display().to_string(),
        source,
    })
}
