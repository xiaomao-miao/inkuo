//! Document scanner - scans workspace for supported documents

use crate::knowledge::config::Document;
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

/// Supported file extensions for knowledge base
const SUPPORTED_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "rs", "js", "ts", "jsx", "tsx", "py", "go", "java", "cpp", "c", "h", "hpp", "json", "toml", "yaml", "yml", "xml", "html", "css", "scss", "vue", "svelte"];

/// Scanner configuration
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// Additional file extensions to include
    pub extra_extensions: Vec<String>,
    /// Directories to exclude
    pub exclude_dirs: Vec<String>,
    /// Max file size in bytes (default 10MB)
    pub max_file_size: usize,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            extra_extensions: Vec::new(),
            exclude_dirs: vec![
                "node_modules".to_string(),
                ".git".to_string(),
                "target".to_string(),
                "dist".to_string(),
                "build".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
                ".idea".to_string(),
                ".vscode".to_string(),
                ".cache".to_string(),
                ".tmp".to_string(),
            ],
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

/// Document scanner
pub struct DocScanner {
    config: ScannerConfig,
}

impl DocScanner {
    pub fn new(config: ScannerConfig) -> Self {
        Self { config }
    }

    /// Scan workspace directory for supported documents
    pub fn scan(&self, workspace_path: &Path) -> Result<Vec<Document>, String> {
        let mut documents = Vec::new();

        for entry in WalkDir::new(workspace_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.should_exclude(e))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Check if file has supported extension
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();

            if !self.is_supported_extension(&extension) {
                continue;
            }

            // Check file size
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() as usize > self.config.max_file_size {
                    tracing::debug!("Skipping large file: {:?}", path);
                    continue;
                }
            }

            // Read file content
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("Failed to read file {:?}: {}", path, e);
                    continue;
                }
            };

            // Compute file hash
            let file_hash = format!("{:x}", Sha256::digest(content.as_bytes()));

            // Generate document ID
            let id = uuid::Uuid::new_v4().to_string();

            // Get relative path and title
            let rel_path = path
                .strip_prefix(workspace_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string();

            documents.push(Document {
                id,
                path: rel_path,
                title,
                content,
                file_hash,
            });
        }

        tracing::info!(
            "Scanned workspace: found {} documents",
            documents.len()
        );

        Ok(documents)
    }

    fn is_supported_extension(&self, ext: &str) -> bool {
        SUPPORTED_EXTENSIONS.iter().any(|e| *e == ext)
            || self.config.extra_extensions.iter().any(|e| e == ext)
    }

    fn should_exclude(&self, entry: &walkdir::DirEntry) -> bool {
        // Exclude hidden directories/files
        let name = entry.file_name().to_string_lossy();
        if name.starts_with('.') {
            return true;
        }

        // Exclude configured directories
        if entry.file_type().is_dir() {
            return self.config.exclude_dirs.contains(&name.to_string());
        }

        false
    }
}

impl Default for DocScanner {
    fn default() -> Self {
        Self::new(ScannerConfig::default())
    }
}
