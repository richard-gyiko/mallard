use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::core::FileStatus;

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub max_file_bytes: u64,
    pub language_allow_list: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WalkEntry {
    pub path: PathBuf,
    pub relative_path: String,
    pub size_bytes: u64,
    pub language: Option<String>,
    pub status: FileStatus,
}

const BINARY_PROBE_BYTES: usize = 8 * 1024;

const SKIP_DIRS: &[&str] = &[".git", ".hg", ".svn", "target", "node_modules"];

pub fn walk(root: &Path, opts: &WalkOptions) -> Vec<WalkEntry> {
    let mut entries: Vec<WalkEntry> = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name();

    for entry in walker.into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        if !e.file_type().is_dir() {
            return true;
        }
        !SKIP_DIRS.iter().any(|s| e.file_name() == *s)
    }) {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file_type = entry.file_type();

        if file_type.is_symlink() {
            if let Some(rel) = relative_path(root, path) {
                entries.push(WalkEntry {
                    path: path.to_path_buf(),
                    relative_path: rel,
                    size_bytes: 0,
                    language: None,
                    status: FileStatus::SkippedSymlink,
                });
            }
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let Some(rel) = relative_path(root, path) else {
            continue;
        };

        let Ok(meta) = entry.metadata() else { continue };
        let size_bytes = meta.len();

        let language = detect_language(path, &opts.language_allow_list);
        if language.is_none() {
            entries.push(WalkEntry {
                path: path.to_path_buf(),
                relative_path: rel,
                size_bytes,
                language: None,
                status: FileStatus::SkippedExtension,
            });
            continue;
        }

        if size_bytes > opts.max_file_bytes {
            entries.push(WalkEntry {
                path: path.to_path_buf(),
                relative_path: rel,
                size_bytes,
                language,
                status: FileStatus::SkippedSize,
            });
            continue;
        }

        if is_binary(path) {
            entries.push(WalkEntry {
                path: path.to_path_buf(),
                relative_path: rel,
                size_bytes,
                language,
                status: FileStatus::SkippedBinary,
            });
            continue;
        }

        entries.push(WalkEntry {
            path: path.to_path_buf(),
            relative_path: rel,
            size_bytes,
            language,
            status: FileStatus::Indexed,
        });
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    entries
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    Some(s)
}

fn detect_language(path: &Path, allow_list: &[String]) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let language = match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "ts" => "typescript",
        "tsx" => "tsx",
        _ => return None,
    };
    if allow_list.is_empty() || allow_list.iter().any(|l| l == language) {
        Some(language.to_string())
    } else {
        None
    }
}

fn is_binary(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buf = [0u8; BINARY_PROBE_BYTES];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    buf[..n].contains(&0)
}
