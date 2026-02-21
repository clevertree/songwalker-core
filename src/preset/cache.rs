use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// Disk cache for library indexes and preset data.
///
/// Structure:
/// ```text
/// {cache_dir}/songwalker/
/// ├── indexes/
/// │   ├── root_index.json
/// │   └── {library}/index.json
/// ├── presets/
/// │   └── {library}/{path}/
/// │       ├── preset.json
/// │       └── samples/
/// │           ├── {sha256}.pcm
/// │           └── {sha256}.meta
/// └── offline/
///     └── {library}.complete
/// ```
pub struct DiskCache {
    base_dir: PathBuf,
}

impl DiskCache {
    /// Create a new disk cache using the platform-appropriate cache directory.
    pub fn new() -> Self {
        let base = if let Some(dirs) = directories::ProjectDirs::from("org", "songwalker", "SongWalker") {
            dirs.cache_dir().to_path_buf()
        } else {
            // Fallback
            PathBuf::from(".songwalker-cache")
        };

        Self { base_dir: base }
    }

    /// Create a disk cache rooted at a specific directory (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { base_dir: path }
    }

    /// Ensure all cache subdirectories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.indexes_dir())?;
        fs::create_dir_all(self.presets_dir())?;
        fs::create_dir_all(self.offline_dir())?;
        Ok(())
    }

    // --- Directory helpers ---

    fn indexes_dir(&self) -> PathBuf {
        self.base_dir.join("indexes")
    }

    fn presets_dir(&self) -> PathBuf {
        self.base_dir.join("presets")
    }

    fn offline_dir(&self) -> PathBuf {
        self.base_dir.join("offline")
    }

    /// Normalize a preset path by stripping .json extension if present.
    /// "instruments/piano/preset.json" -> "instruments/piano/preset"
    fn normalize_preset_path(preset_path: &str) -> &str {
        preset_path.strip_suffix(".json").unwrap_or(preset_path)
    }

    // --- Root index ---

    pub fn root_index_path(&self) -> PathBuf {
        self.indexes_dir().join("root_index.json")
    }

    pub fn read_root_index(&self) -> Option<String> {
        fs::read_to_string(self.root_index_path()).ok()
    }

    pub fn write_root_index(&self, data: &str) -> std::io::Result<()> {
        let path = self.root_index_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)
    }

    // --- Library indexes ---

    pub fn library_index_path(&self, library: &str) -> PathBuf {
        self.indexes_dir().join(library).join("index.json")
    }

    pub fn read_library_index(&self, library: &str) -> Option<String> {
        fs::read_to_string(self.library_index_path(library)).ok()
    }

    pub fn write_library_index(&self, library: &str, data: &str) -> std::io::Result<()> {
        let path = self.library_index_path(library);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)
    }

    // --- Preset descriptors ---

    /// Get the path for a cached preset descriptor file.
    /// Normalizes the path by stripping .json suffix if present, then adds .json back.
    /// This ensures consistent cache paths regardless of whether caller includes .json.
    pub fn preset_path(&self, library: &str, preset_path: &str) -> PathBuf {
        let normalized = Self::normalize_preset_path(preset_path);
        self.presets_dir()
            .join(library)
            .join(format!("{}.json", normalized))
    }

    pub fn read_preset(&self, library: &str, preset_path: &str) -> Option<String> {
        fs::read_to_string(self.preset_path(library, preset_path)).ok()
    }

    pub fn write_preset(
        &self,
        library: &str,
        preset_path: &str,
        data: &str,
    ) -> std::io::Result<()> {
        let path = self.preset_path(library, preset_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)
    }

    // --- Sample data ---

    /// Get the path for a cached sample file (decoded PCM).
    /// Uses normalized preset path (without .json) as base directory for samples.
    pub fn sample_path(&self, library: &str, preset_path: &str, url_or_hash: &str) -> PathBuf {
        let hash = sha256_hex(url_or_hash);
        let normalized = Self::normalize_preset_path(preset_path);
        self.presets_dir()
            .join(library)
            .join(normalized)
            .join("samples")
            .join(format!("{}.pcm", hash))
    }

    /// Read cached PCM sample data.
    pub fn read_sample(&self, library: &str, preset_path: &str, url_or_hash: &str) -> Option<Vec<f32>> {
        let path = self.sample_path(library, preset_path, url_or_hash);
        let bytes = fs::read(&path).ok()?;
        // PCM data is stored as raw f32 little-endian
        if bytes.len() % 4 != 0 || bytes.is_empty() {
            // Empty or malformed cache file — treat as cache miss so it re-fetches
            return None;
        }
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        Some(samples)
    }

    /// Write decoded PCM sample data to cache.
    pub fn write_sample(
        &self,
        library: &str,
        preset_path: &str,
        url_or_hash: &str,
        samples: &[f32],
    ) -> std::io::Result<()> {
        if samples.is_empty() {
            // Never cache empty sample data — it would poison the cache
            return Ok(());
        }
        let path = self.sample_path(library, preset_path, url_or_hash);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        fs::write(&path, &bytes)
    }

    // --- Offline markers ---

    /// Check if a library has been fully downloaded for offline use.
    pub fn is_offline(&self, library: &str) -> bool {
        self.offline_dir()
            .join(format!("{}.complete", library))
            .exists()
    }

    /// Mark a library as fully downloaded for offline use.
    pub fn mark_offline(&self, library: &str) -> std::io::Result<()> {
        let path = self.offline_dir().join(format!("{}.complete", library));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, "")
    }

    // --- Cache size ---

    /// Calculate total cache size on disk in bytes.
    pub fn total_size_bytes(&self) -> u64 {
        dir_size(&self.base_dir)
    }
}

/// Compute SHA-256 hex digest of a string.
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Encode bytes as hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Recursively compute directory size.
fn dir_size(path: &PathBuf) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(meta) = meta {
                if meta.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}
