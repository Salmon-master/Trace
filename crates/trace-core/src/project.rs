use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::CONFIG_FILENAME;

#[derive(Debug, Default)]
pub struct KicadCandidates {
    pub projects: Vec<PathBuf>,
    pub schematics: Vec<PathBuf>,
    pub pcbs: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("failed to inspect {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("no KiCad schematic was found under {root}")]
    NoSchematic { root: String },
    #[error("multiple KiCad {kind} files were found:\n{paths}")]
    Multiple { kind: &'static str, paths: String },
}

#[derive(Debug)]
pub struct DiscoveredKicadFiles {
    pub project: Option<PathBuf>,
    pub schematic: PathBuf,
    pub pcb: Option<PathBuf>,
}

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

pub fn discover_kicad_files(
    root: impl AsRef<Path>,
) -> Result<DiscoveredKicadFiles, DiscoveryError> {
    let root = root.as_ref();
    let mut candidates = KicadCandidates::default();
    visit_directory(root, &mut candidates)?;

    let schematic = select_one("schematic", &candidates.schematics, || {
        DiscoveryError::NoSchematic {
            root: root.display().to_string(),
        }
    })?;

    Ok(DiscoveredKicadFiles {
        project: select_optional("project", &candidates.projects)?,
        schematic,
        pcb: select_optional("PCB", &candidates.pcbs)?,
    })
}

fn visit_directory(
    directory: &Path,
    candidates: &mut KicadCandidates,
) -> Result<(), DiscoveryError> {
    let entries = std::fs::read_dir(directory).map_err(|source| DiscoveryError::Io {
        path: directory.display().to_string(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| DiscoveryError::Io {
            path: directory.display().to_string(),
            source,
        })?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if path.is_dir() {
            if is_ignored_directory(&file_name) {
                continue;
            }
            visit_directory(&path, candidates)?;
            continue;
        }

        match path.extension().and_then(|extension| extension.to_str()) {
            Some("kicad_pro") => candidates.projects.push(path),
            Some("kicad_sch") => candidates.schematics.push(path),
            Some("kicad_pcb") => candidates.pcbs.push(path),
            _ => {}
        }
    }

    Ok(())
}

fn is_ignored_directory(name: &str) -> bool {
    matches!(name, ".git" | ".trace" | "target" | ".history")
        || name.starts_with("_restore_backup_")
}

fn select_optional(
    kind: &'static str,
    paths: &[PathBuf],
) -> Result<Option<PathBuf>, DiscoveryError> {
    match paths {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => Err(DiscoveryError::Multiple {
            kind,
            paths: format_paths(paths),
        }),
    }
}

fn select_one<F>(
    kind: &'static str,
    paths: &[PathBuf],
    missing: F,
) -> Result<PathBuf, DiscoveryError>
where
    F: FnOnce() -> DiscoveryError,
{
    match paths {
        [] => Err(missing()),
        [path] => Ok(path.clone()),
        _ => Err(DiscoveryError::Multiple {
            kind,
            paths: format_paths(paths),
        }),
    }
}

fn format_paths(paths: &[PathBuf]) -> String {
    let mut paths = paths.to_vec();
    paths.sort();
    paths
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}
