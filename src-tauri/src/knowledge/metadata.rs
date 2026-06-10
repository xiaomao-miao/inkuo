//! Metadata module - manages workspace knowledge base metadata
//!
//! Stores information about indexed files for incremental updates.

use crate::knowledge::config::Document;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Indexed file record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    /// File relative path
    pub path: String,
    /// Content hash
    pub hash: String,
    /// Number of chunks
    pub chunk_count: usize,
    /// When this file was indexed
    pub indexed_at: DateTime<Utc>,
}

/// Knowledge base metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeMetadata {
    /// Workspace unique identifier
    pub workspace_id: String,
    /// Original workspace path
    pub workspace_path: String,
    /// Collection name
    pub collection_name: String,
    /// When the knowledge base was created
    pub created_at: DateTime<Utc>,
    /// Last update time
    pub last_updated: DateTime<Utc>,
    /// Number of documents
    pub document_count: usize,
    /// Number of chunks
    pub chunk_count: usize,
    /// Indexed files
    pub indexed_files: Vec<IndexedFile>,
    /// Explicitly selected member file paths (relative to workspace)
    #[serde(default)]
    pub members: Vec<String>,
}

/// Metadata store for a workspace
pub struct MetadataStore {
    /// Storage directory
    pub dir: PathBuf,
    /// Cached metadata
    pub metadata: Option<KnowledgeMetadata>,
}

impl MetadataStore {
    /// Create a new metadata store for a workspace
    pub fn new(workspace_path: &Path) -> Result<Self, MetadataError> {
        let workspace_id = Self::hash_workspace_path(workspace_path);
        let dir = get_knowledge_dir()
            .map(|p| p.join(&workspace_id))
            .unwrap_or_else(|| {
                PathBuf::from(format!("/tmp/inkuo_knowledge_{}", &workspace_id[..8]))
            });

        std::fs::create_dir_all(&dir)
            .map_err(|e| MetadataError::Io(format!("Failed to create directory: {}", e)))?;

        Ok(Self {
            dir,
            metadata: None,
        })
    }

    /// Generate a unique ID for the workspace
    fn hash_workspace_path(path: &Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut s = DefaultHasher::new();
        abs_path.to_string_lossy().hash(&mut s);
        format!("{:x}", s.finish())
    }

    /// Get the metadata file path
    pub fn metadata_path(&self) -> PathBuf {
        self.dir.join("metadata.json")
    }

    /// Check if metadata exists
    pub fn exists(&self) -> bool {
        self.metadata_path().exists()
    }

    /// Load metadata from disk
    pub fn load(&mut self) -> Result<&KnowledgeMetadata, MetadataError> {
        let path = self.metadata_path();
        if !path.exists() {
            return Err(MetadataError::NotFound);
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| MetadataError::Io(format!("Failed to read metadata: {}", e)))?;

        let metadata: KnowledgeMetadata = serde_json::from_str(&content)
            .map_err(|e| MetadataError::Parse(format!("Failed to parse metadata: {}", e)))?;

        self.metadata = Some(metadata);
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Save metadata to disk
    pub fn save(&self) -> Result<(), MetadataError> {
        let metadata = self.metadata.as_ref()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let path = self.metadata_path();
        let content = serde_json::to_string_pretty(metadata)
            .map_err(|e| MetadataError::Serialize(format!("Failed to serialize: {}", e)))?;

        std::fs::write(&path, content)
            .map_err(|e| MetadataError::Io(format!("Failed to write metadata: {}", e)))?;

        Ok(())
    }

    /// Create new metadata
    pub fn create(&mut self, workspace_path: &Path, collection_name: &str) -> Result<(), MetadataError> {
        let workspace_id = Self::hash_workspace_path(workspace_path);

        self.metadata = Some(KnowledgeMetadata {
            workspace_id,
            workspace_path: workspace_path.to_string_lossy().to_string(),
            collection_name: collection_name.to_string(),
            created_at: Utc::now(),
            last_updated: Utc::now(),
            document_count: 0,
            chunk_count: 0,
            indexed_files: Vec::new(),
            members: Vec::new(),
        });

        self.save()
    }

    /// Update metadata with new documents (preserves members)
    pub fn update(&mut self, documents: &[Document], chunk_count: usize) -> Result<(), MetadataError> {
        let metadata = self.metadata.as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        // Update indexed files
        let indexed_files: Vec<IndexedFile> = documents
            .iter()
            .map(|doc| IndexedFile {
                path: doc.path.clone(),
                hash: doc.file_hash.clone(),
                chunk_count: 1, // Simplified
                indexed_at: Utc::now(),
            })
            .collect();

        metadata.indexed_files = indexed_files;
        metadata.document_count = documents.len();
        metadata.chunk_count = chunk_count;
        metadata.last_updated = Utc::now();

        self.save()
    }

    /// Add a member file path to the knowledge base
    pub fn add_member(&mut self, member_path: &str) -> Result<bool, MetadataError> {
        let metadata = self.metadata.as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        if metadata.members.contains(&member_path.to_string()) {
            return Ok(false);
        }

        metadata.members.push(member_path.to_string());
        metadata.last_updated = Utc::now();
        self.save()?;
        Ok(true)
    }

    /// Remove a member file path from the knowledge base
    pub fn remove_member(&mut self, member_path: &str) -> Result<bool, MetadataError> {
        let metadata = self.metadata.as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let original_len = metadata.members.len();
        metadata.members.retain(|p| p != member_path);

        if metadata.members.len() == original_len {
            return Ok(false);
        }

        metadata.last_updated = Utc::now();
        self.save()?;
        Ok(true)
    }

    /// Add multiple member file paths at once
    pub fn add_members(&mut self, member_paths: &[String]) -> Result<usize, MetadataError> {
        let metadata = self.metadata.as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let mut added = 0;
        for path in member_paths {
            if !metadata.members.contains(path) {
                metadata.members.push(path.clone());
                added += 1;
            }
        }

        if added > 0 {
            metadata.last_updated = Utc::now();
            self.save()?;
        }

        Ok(added)
    }

    /// Remove multiple member file paths at once
    pub fn remove_members(&mut self, member_paths: &[String]) -> Result<usize, MetadataError> {
        let metadata = self.metadata.as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let original_len = metadata.members.len();
        metadata.members.retain(|p| !member_paths.contains(p));

        let removed = original_len - metadata.members.len();
        if removed > 0 {
            metadata.last_updated = Utc::now();
            self.save()?;
        }

        Ok(removed)
    }

    /// Check if a path is a member
    pub fn is_member(&self, member_path: &str) -> bool {
        self.metadata.as_ref()
            .map(|m| m.members.contains(&member_path.to_string()))
            .unwrap_or(false)
    }

    /// Get indexed files as a HashMap for quick lookup
    pub fn get_indexed_files_map(&self) -> HashMap<String, &IndexedFile> {
        let metadata = match &self.metadata {
            Some(m) => m,
            None => return HashMap::new(),
        };

        metadata
            .indexed_files
            .iter()
            .map(|f| (f.path.clone(), f))
            .collect()
    }

    /// Find changed files compared to current state
    pub fn find_changed_files<'a>(
        &self,
        current_docs: &'a [Document],
    ) -> (Vec<&'a Document>, Vec<String>) {
        let indexed = self.get_indexed_files_map();
        let mut changed = Vec::new();
        let mut removed = Vec::new();

        for doc in current_docs {
            match indexed.get(&doc.path) {
                Some(indexed_file) if indexed_file.hash != doc.file_hash => {
                    // File changed
                    changed.push(doc);
                }
                None => {
                    // New file
                    changed.push(doc);
                }
                Some(_) => {
                    // Unchanged, skip
                }
            }
        }

        // Find removed files
        let current_paths: std::collections::HashSet<_> = current_docs
            .iter()
            .map(|d| d.path.as_str())
            .collect();

        for (path, _) in &indexed {
            if !current_paths.contains(path.as_str()) {
                removed.push(path.clone());
            }
        }

        (changed, removed)
    }

    /// Get the metadata (must call load or create first)
    pub fn get_metadata(&self) -> Option<&KnowledgeMetadata> {
        self.metadata.as_ref()
    }
}

/// Metadata error
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Metadata not found")]
    NotFound,
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Serialization error: {0}")]
    Serialize(String),
    #[error("No metadata loaded")]
    NoMetadata,
}

/// Get the knowledge base directory
fn get_knowledge_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|p| p.join("inkuo").join("knowledge"))
}
