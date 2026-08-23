//! Metadata module - manages workspace knowledge base metadata
//!
//! Stores information about indexed files for incremental updates.

use crate::knowledge::config::{default_collection, Document, ImportFailure};
use crate::knowledge::scanner::normalize_collection;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Indexed file record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    /// Document ID (matches `Document.id`, used as the `document_id` payload
    /// field in Qdrant). `#[serde(default)]` keeps loading old `metadata.json`
    /// files that predate this field.
    #[serde(default)]
    pub document_id: String,
    /// File relative path
    pub path: String,
    /// Content hash
    pub hash: String,
    /// Number of chunks
    pub chunk_count: usize,
    /// When this file was indexed
    pub indexed_at: DateTime<Utc>,
    #[serde(default = "default_collection")]
    pub collection: String,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFailureRecord {
    pub path: String,
    pub error: String,
    #[serde(default = "default_collection")]
    pub collection: String,
    pub attempted_at: DateTime<Utc>,
}

/// Old Qdrant generation that must be retired after metadata has switched to
/// its staged replacement. Persisting this queue closes the crash window
/// between metadata commit and vector cleanup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingRetirement {
    pub document_id: String,
    #[serde(default = "default_collection")]
    pub collection: String,
    pub path: String,
    pub queued_at: DateTime<Utc>,
}

impl PendingRetirement {
    pub fn new(
        collection: impl Into<String>,
        path: impl Into<String>,
        document_id: impl Into<String>,
    ) -> Self {
        Self {
            document_id: document_id.into(),
            collection: collection.into(),
            path: path.into(),
            queued_at: Utc::now(),
        }
    }
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
    /// Named collections. Old metadata only had `members`; on load those
    /// paths migrate into the `default` collection automatically.
    #[serde(default)]
    pub collections: HashMap<String, Vec<String>>,
    /// Last per-file import diagnostics, shown in the knowledge manager.
    #[serde(default)]
    pub failures: Vec<IndexFailureRecord>,
    /// Durable cleanup queue for previous vector generations. Old metadata
    /// deserializes with an empty queue.
    #[serde(default)]
    pub pending_retirements: Vec<PendingRetirement>,
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

    // ── Lifecycle helpers ─────────────────────────────────────────────────────────

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

        let mut metadata: KnowledgeMetadata = serde_json::from_str(&content)
            .map_err(|e| MetadataError::Parse(format!("Failed to parse metadata: {}", e)))?;

        normalize_loaded_metadata(&mut metadata);

        self.metadata = Some(metadata);
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Save metadata to disk
    pub fn save(&self) -> Result<(), MetadataError> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let path = self.metadata_path();
        let content = serde_json::to_string_pretty(metadata)
            .map_err(|e| MetadataError::Serialize(format!("Failed to serialize: {}", e)))?;

        atomic_write_metadata(&path, content.as_bytes())
            .map_err(|e| MetadataError::Io(format!("Failed to write metadata: {}", e)))?;

        Ok(())
    }

    /// Create new metadata
    pub fn create(
        &mut self,
        workspace_path: &Path,
        collection_name: &str,
    ) -> Result<(), MetadataError> {
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
            collections: HashMap::from([(default_collection(), Vec::new())]),
            failures: Vec::new(),
            pending_retirements: Vec::new(),
        });

        self.save()
    }

    /// Update metadata with new documents (preserves members).
    ///
    /// `documents` is the list of documents that were actually (re-)indexed in
    /// this batch — typically the `changed_docs` returned from
    /// `find_changed_files`. Previously this method *replaced* the entire
    /// `indexed_files` list with this set, which silently wiped metadata for
    /// unchanged files on every incremental update. It now merges:
    ///
    ///   - records for paths in `documents` are updated in place (or appended
    ///     as new);
    ///   - records for paths in `removed_paths` are dropped;
    ///   - records for any other path are preserved untouched.
    ///
    /// `chunk_count_by_doc` lets the caller supply the real per-document
    /// chunk counts; the previous hard-coded `chunk_count: 1` made the stored
    /// chunk_count useless for diagnostics.

    // ── Update / membership ────────────────────────────────────────────────────────

    pub fn update(
        &mut self,
        documents: &[Document],
        chunk_count_by_doc: &HashMap<String, usize>,
        _total_chunk_count: usize,
        removed_paths: &[String],
    ) -> Result<(), MetadataError> {
        self.update_with_retirements(
            documents,
            chunk_count_by_doc,
            _total_chunk_count,
            removed_paths,
            &[],
        )
    }

    /// Atomically switch indexed document rows and enqueue their previous
    /// generations for post-commit cleanup in the same metadata write.
    pub fn update_with_retirements(
        &mut self,
        documents: &[Document],
        chunk_count_by_doc: &HashMap<String, usize>,
        _total_chunk_count: usize,
        removed_paths: &[String],
        retirements: &[PendingRetirement],
    ) -> Result<(), MetadataError> {
        let metadata = self
            .metadata
            .as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        // 1. Drop records for files that no longer exist / are no longer
        //    members of the knowledge base.
        if !removed_paths.is_empty() {
            let affected_collections: std::collections::HashSet<&str> = documents
                .iter()
                .map(|document| document.collection.as_str())
                .collect();
            // A path may legitimately be indexed in several named
            // collections. Only derive removals when the caller supplied a
            // collection-bearing document batch; an empty/ambiguous batch is
            // safer as a no-op than deleting rows across every collection.
            if !affected_collections.is_empty() {
                metadata.indexed_files.retain(|file| {
                    !affected_collections.contains(file.collection.as_str())
                        || !removed_paths.iter().any(|path| path == &file.path)
                });
            }
        }

        // 2. Upsert per-document records. We touch only what changed in this
        //    batch; the rest of `indexed_files` stays put.
        let now = Utc::now();
        for doc in documents {
            let chunk_count = chunk_count_by_doc.get(&doc.id).copied().unwrap_or(0);
            let entry = IndexedFile {
                document_id: doc.id.clone(),
                path: doc.path.clone(),
                hash: doc.file_hash.clone(),
                chunk_count,
                indexed_at: now,
                collection: doc.collection.clone(),
                source_type: doc.source_type.clone(),
                size_bytes: doc.size_bytes,
            };
            if let Some(existing) = metadata
                .indexed_files
                .iter_mut()
                .find(|f| f.path == doc.path && f.collection == doc.collection)
            {
                *existing = entry;
            } else {
                metadata.indexed_files.push(entry);
            }
        }

        metadata.document_count = metadata.indexed_files.len();
        metadata.chunk_count = metadata
            .indexed_files
            .iter()
            .map(|file| file.chunk_count)
            .sum();
        metadata.last_updated = now;

        for retirement in retirements {
            if retirement.document_id.is_empty()
                || metadata.pending_retirements.iter().any(|pending| {
                    pending.collection == retirement.collection
                        && pending.document_id == retirement.document_id
                })
            {
                continue;
            }
            metadata.pending_retirements.push(retirement.clone());
        }

        self.save()
    }

    /// Add a member file path to the knowledge base
    pub fn add_member(&mut self, member_path: &str) -> Result<bool, MetadataError> {
        self.add_member_to_collection(&default_collection(), member_path)
    }

    pub fn add_member_to_collection(
        &mut self,
        collection: &str,
        member_path: &str,
    ) -> Result<bool, MetadataError> {
        let metadata = self
            .metadata
            .as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let collection_members = metadata
            .collections
            .entry(collection.to_string())
            .or_default();
        if collection_members.iter().any(|path| path == member_path) {
            return Ok(false);
        }

        collection_members.push(member_path.to_string());
        sync_legacy_members(metadata);
        metadata.last_updated = Utc::now();
        self.save()?;
        Ok(true)
    }

    /// Remove a member file path from the knowledge base
    pub fn remove_member(&mut self, member_path: &str) -> Result<bool, MetadataError> {
        self.remove_member_from_collection(&default_collection(), member_path)
    }

    pub fn remove_member_from_collection(
        &mut self,
        collection: &str,
        member_path: &str,
    ) -> Result<bool, MetadataError> {
        let metadata = self
            .metadata
            .as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;

        let Some(collection_members) = metadata.collections.get_mut(collection) else {
            return Ok(false);
        };
        let original_len = collection_members.len();
        collection_members.retain(|path| path != member_path);

        if collection_members.len() == original_len {
            return Ok(false);
        }

        sync_legacy_members(metadata);

        metadata.last_updated = Utc::now();
        self.save()?;
        Ok(true)
    }

    /// Add multiple member file paths at once
    pub fn add_members(&mut self, member_paths: &[String]) -> Result<usize, MetadataError> {
        self.add_members_to_collection(&default_collection(), member_paths)
    }

    pub fn add_members_to_collection(
        &mut self,
        collection: &str,
        member_paths: &[String],
    ) -> Result<usize, MetadataError> {
        let metadata = self
            .metadata
            .as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;
        let collection_members = metadata
            .collections
            .entry(collection.to_string())
            .or_default();
        let mut added = 0;
        for path in member_paths {
            if !collection_members.contains(path) {
                collection_members.push(path.clone());
                added += 1;
            }
        }

        if added > 0 {
            sync_legacy_members(metadata);
            metadata.last_updated = Utc::now();
            self.save()?;
        }

        Ok(added)
    }

    /// Remove multiple member file paths at once
    pub fn remove_members(&mut self, member_paths: &[String]) -> Result<usize, MetadataError> {
        self.remove_members_from_collection(&default_collection(), member_paths)
    }

    pub fn remove_members_from_collection(
        &mut self,
        collection: &str,
        member_paths: &[String],
    ) -> Result<usize, MetadataError> {
        let metadata = self
            .metadata
            .as_mut()
            .ok_or_else(|| MetadataError::NoMetadata)?;
        let Some(collection_members) = metadata.collections.get_mut(collection) else {
            return Ok(0);
        };

        let original_len = collection_members.len();
        collection_members.retain(|path| !member_paths.contains(path));

        let removed = original_len - collection_members.len();
        if removed > 0 {
            sync_legacy_members(metadata);
            metadata.last_updated = Utc::now();
            self.save()?;
        }

        Ok(removed)
    }

    /// Check if a path is a member
    pub fn is_member(&self, member_path: &str) -> bool {
        self.is_member_of_collection(&default_collection(), member_path)
    }

    pub fn is_member_of_collection(&self, collection: &str, member_path: &str) -> bool {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.collections.get(collection))
            .map(|members| members.iter().any(|path| path == member_path))
            .unwrap_or(false)
    }

    /// Get indexed files as a HashMap for quick lookup
    pub fn get_indexed_files_map(&self) -> HashMap<String, &IndexedFile> {
        self.get_indexed_files_map_for_collection(&default_collection())
    }

    pub fn get_indexed_files_map_for_collection(
        &self,
        collection: &str,
    ) -> HashMap<String, &IndexedFile> {
        let metadata = match &self.metadata {
            Some(m) => m,
            None => return HashMap::new(),
        };

        metadata
            .indexed_files
            .iter()
            .filter(|file| file.collection == collection)
            .map(|f| (f.path.clone(), f))
            .collect()
    }

    /// Find changed files compared to current state
    pub fn find_changed_files<'a>(
        &self,
        current_docs: &'a [Document],
    ) -> (Vec<&'a Document>, Vec<String>) {
        let collection = current_docs
            .first()
            .map(|document| document.collection.as_str())
            .unwrap_or("default");
        let indexed = self.get_indexed_files_map_for_collection(collection);
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
        let current_paths: std::collections::HashSet<_> =
            current_docs.iter().map(|d| d.path.as_str()).collect();

        for (path, _) in &indexed {
            if !current_paths.contains(path.as_str()) {
                removed.push(path.clone());
            }
        }

        (changed, removed)
    }

    pub fn members_for_collection(&self, collection: &str) -> Vec<String> {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.collections.get(collection))
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_collection_members(
        &mut self,
        collection: &str,
        members: Vec<String>,
    ) -> Result<(), MetadataError> {
        let metadata = self.metadata.as_mut().ok_or(MetadataError::NoMetadata)?;
        metadata
            .collections
            .insert(collection.to_string(), dedupe_strings(members));
        sync_legacy_members(metadata);
        metadata.last_updated = Utc::now();
        self.save()
    }

    pub fn remove_indexed_files(
        &mut self,
        collection: &str,
        paths: &[String],
    ) -> Result<Vec<String>, MetadataError> {
        let metadata = self.metadata.as_mut().ok_or(MetadataError::NoMetadata)?;
        let mut document_ids = Vec::new();
        metadata.indexed_files.retain(|file| {
            let should_remove =
                file.collection == collection && paths.iter().any(|path| path == &file.path);
            if should_remove && !file.document_id.is_empty() {
                document_ids.push(file.document_id.clone());
            }
            !should_remove
        });
        metadata.document_count = metadata.indexed_files.len();
        metadata.chunk_count = metadata
            .indexed_files
            .iter()
            .map(|file| file.chunk_count)
            .sum();
        metadata.last_updated = Utc::now();
        self.save()?;
        Ok(document_ids)
    }

    /// Return the persisted document ids for `paths` without mutating or
    /// saving metadata. Callers use this to delete vectors first and only
    /// commit the metadata removal after the vector operation succeeds. That
    /// ordering prevents a failed shard write from leaving searchable
    /// "ghost" vectors with no corresponding metadata row.
    pub fn indexed_document_ids_for_paths(
        &self,
        collection: &str,
        paths: &[String],
    ) -> Vec<String> {
        let Some(metadata) = self.metadata.as_ref() else {
            return Vec::new();
        };
        metadata
            .indexed_files
            .iter()
            .filter(|file| {
                file.collection == collection
                    && paths.iter().any(|path| path == &file.path)
                    && !file.document_id.is_empty()
            })
            .map(|file| file.document_id.clone())
            .collect()
    }

    pub fn record_failures(
        &mut self,
        collection: &str,
        failures: &[ImportFailure],
    ) -> Result<(), MetadataError> {
        let metadata = self.metadata.as_mut().ok_or(MetadataError::NoMetadata)?;
        let attempted_paths: std::collections::HashSet<&str> = failures
            .iter()
            .map(|failure| failure.path.as_str())
            .collect();
        metadata.failures.retain(|failure| {
            failure.collection != collection || !attempted_paths.contains(failure.path.as_str())
        });
        let now = Utc::now();
        metadata
            .failures
            .extend(failures.iter().map(|failure| IndexFailureRecord {
                path: failure.path.clone(),
                error: failure.error.clone(),
                collection: collection.to_string(),
                attempted_at: now,
            }));
        metadata.last_updated = now;
        self.save()
    }

    pub fn clear_failures_for_paths(
        &mut self,
        collection: &str,
        paths: &[String],
    ) -> Result<(), MetadataError> {
        let metadata = self.metadata.as_mut().ok_or(MetadataError::NoMetadata)?;
        metadata.failures.retain(|failure| {
            failure.collection != collection || !paths.iter().any(|path| path == &failure.path)
        });
        self.save()
    }

    pub fn pending_retirements_for_collection(&self, collection: &str) -> Vec<PendingRetirement> {
        self.metadata
            .as_ref()
            .map(|metadata| {
                metadata
                    .pending_retirements
                    .iter()
                    .filter(|pending| pending.collection == collection)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn clear_pending_retirements(
        &mut self,
        collection: &str,
        document_ids: &[String],
    ) -> Result<(), MetadataError> {
        if document_ids.is_empty() {
            return Ok(());
        }
        let metadata = self.metadata.as_mut().ok_or(MetadataError::NoMetadata)?;
        metadata.pending_retirements.retain(|pending| {
            pending.collection != collection || !document_ids.contains(&pending.document_id)
        });
        metadata.last_updated = Utc::now();
        self.save()
    }

    /// Get the metadata (must call load or create first)
    pub fn get_metadata(&self) -> Option<&KnowledgeMetadata> {
        self.metadata.as_ref()
    }
}

fn atomic_write_metadata(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("cannot determine metadata parent for {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("metadata.json"),
        uuid::Uuid::new_v4()
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    if let Err(error) = replace_staged_metadata(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    sync_parent_directory(parent);
    Ok(())
}

fn replace_staged_metadata(staged: &Path, destination: &Path) -> std::io::Result<()> {
    match std::fs::rename(staged, destination) {
        Ok(()) => return Ok(()),
        Err(primary_error) if !destination.exists() => return Err(primary_error),
        Err(_) => {}
    }

    // Windows does not replace an existing file with `rename`. Keep the old
    // metadata under a unique sibling until activation succeeds, and restore
    // it if the second rename fails.
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let backup = parent.join(format!(
        ".{}-backup-{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("metadata.json"),
        uuid::Uuid::new_v4()
    ));
    std::fs::rename(destination, &backup)?;
    if let Err(activation_error) = std::fs::rename(staged, destination) {
        return match std::fs::rename(&backup, destination) {
            Ok(()) => Err(activation_error),
            Err(restore_error) => Err(std::io::Error::new(
                restore_error.kind(),
                format!(
                    "activate metadata failed: {}; restore previous metadata from {} failed: {}",
                    activation_error,
                    backup.display(),
                    restore_error
                ),
            )),
        };
    }
    if let Err(error) = std::fs::remove_file(&backup) {
        tracing::warn!(
            "Metadata replacement succeeded but backup {} could not be removed: {}",
            backup.display(),
            error
        );
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) {
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) {}

fn normalize_loaded_metadata(metadata: &mut KnowledgeMetadata) {
    if metadata.collections.is_empty() {
        metadata.collections.insert(
            default_collection(),
            dedupe_strings(metadata.members.clone()),
        );
    }

    let mut canonical_collections: HashMap<String, Vec<String>> = HashMap::new();
    for (collection, members) in std::mem::take(&mut metadata.collections) {
        canonical_collections
            .entry(normalize_collection(&collection))
            .or_default()
            .extend(members);
    }
    for members in canonical_collections.values_mut() {
        *members = dedupe_strings(std::mem::take(members));
    }
    metadata.collections = canonical_collections;

    for file in &mut metadata.indexed_files {
        file.collection = normalize_collection(&file.collection);
    }
    for failure in &mut metadata.failures {
        failure.collection = normalize_collection(&failure.collection);
    }
    for pending in &mut metadata.pending_retirements {
        pending.collection = normalize_collection(&pending.collection);
    }
    metadata
        .pending_retirements
        .retain(|pending| !pending.document_id.trim().is_empty());
    let mut seen_retirements = std::collections::HashSet::new();
    metadata.pending_retirements.retain(|pending| {
        seen_retirements.insert((pending.collection.clone(), pending.document_id.clone()))
    });
    sync_legacy_members(metadata);
    metadata.document_count = metadata.indexed_files.len();
    metadata.chunk_count = metadata
        .indexed_files
        .iter()
        .map(|file| file.chunk_count)
        .sum();
}

fn sync_legacy_members(metadata: &mut KnowledgeMetadata) {
    let mut all = Vec::new();
    let mut collections: Vec<_> = metadata.collections.iter().collect();
    collections.sort_by(|a, b| a.0.cmp(b.0));
    for (_, members) in collections {
        for path in members {
            if !all.contains(path) {
                all.push(path.clone());
            }
        }
    }
    metadata.members = all;
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        if !output.contains(&value) {
            output.push(value);
        }
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_members_migrate_to_default_collection() {
        let now = Utc::now();
        let mut metadata = KnowledgeMetadata {
            workspace_id: "legacy".into(),
            workspace_path: "/tmp/workspace".into(),
            collection_name: "kb_legacy".into(),
            created_at: now,
            last_updated: now,
            document_count: 99,
            chunk_count: 99,
            indexed_files: vec![IndexedFile {
                document_id: "doc-1".into(),
                path: "notes.md".into(),
                hash: "abc".into(),
                chunk_count: 3,
                indexed_at: now,
                collection: String::new(),
                source_type: String::new(),
                size_bytes: 0,
            }],
            members: vec!["notes.md".into(), "notes.md".into()],
            collections: HashMap::new(),
            failures: Vec::new(),
            pending_retirements: Vec::new(),
        };

        normalize_loaded_metadata(&mut metadata);
        assert_eq!(
            metadata.collections.get("default").unwrap(),
            &vec!["notes.md".to_string()]
        );
        assert_eq!(metadata.indexed_files[0].collection, "default");
        assert_eq!(metadata.document_count, 1);
        assert_eq!(metadata.chunk_count, 3);
    }

    #[test]
    fn old_metadata_json_deserializes_with_collection_defaults() {
        let now = Utc::now().to_rfc3339();
        let json = serde_json::json!({
            "workspace_id": "legacy-json",
            "workspace_path": "/tmp/workspace",
            "collection_name": "kb_legacy",
            "created_at": now,
            "last_updated": now,
            "document_count": 1,
            "chunk_count": 2,
            "indexed_files": [{
                "path": "paper.pdf",
                "hash": "abc",
                "chunk_count": 2,
                "indexed_at": now
            }],
            "members": ["paper.pdf"]
        });
        let mut metadata: KnowledgeMetadata = serde_json::from_value(json).unwrap();
        normalize_loaded_metadata(&mut metadata);
        assert_eq!(metadata.collections["default"], vec!["paper.pdf"]);
        assert_eq!(metadata.indexed_files[0].collection, "default");
        assert!(metadata.failures.is_empty());
        assert!(metadata.pending_retirements.is_empty());
    }

    #[test]
    fn legacy_members_remain_a_deduplicated_union_of_collections() {
        let now = Utc::now();
        let mut metadata = KnowledgeMetadata {
            workspace_id: "current".into(),
            workspace_path: "/tmp/workspace".into(),
            collection_name: "kb_current".into(),
            created_at: now,
            last_updated: now,
            document_count: 0,
            chunk_count: 0,
            indexed_files: Vec::new(),
            members: Vec::new(),
            collections: HashMap::from([
                ("default".into(), vec!["a.md".into()]),
                ("research".into(), vec!["a.md".into(), "b.pdf".into()]),
            ]),
            failures: Vec::new(),
            pending_retirements: Vec::new(),
        };
        normalize_loaded_metadata(&mut metadata);
        assert_eq!(metadata.members.len(), 2);
        assert!(metadata.members.contains(&"a.md".to_string()));
        assert!(metadata.members.contains(&"b.pdf".to_string()));
        assert_eq!(
            metadata.collections.get("default").unwrap(),
            &vec!["a.md".to_string()]
        );

        // Simulate the persisted JSON round trip. `members` is a compatibility
        // union and must never be merged back into the default collection.
        let serialized = serde_json::to_string(&metadata).unwrap();
        let mut reloaded: KnowledgeMetadata = serde_json::from_str(&serialized).unwrap();
        normalize_loaded_metadata(&mut reloaded);
        assert_eq!(
            reloaded.collections.get("default").unwrap(),
            &vec!["a.md".to_string()]
        );
        assert_eq!(
            reloaded.collections.get("research").unwrap(),
            &vec!["a.md".to_string(), "b.pdf".to_string()]
        );
    }

    #[test]
    fn update_removal_is_scoped_to_the_document_collection() {
        let now = Utc::now();
        let dir = std::env::temp_dir().join(format!("inkuo-kb-metadata-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let shared = |collection: &str, id: &str| IndexedFile {
            document_id: id.into(),
            path: "shared.md".into(),
            hash: "old".into(),
            chunk_count: 1,
            indexed_at: now,
            collection: collection.into(),
            source_type: "markdown".into(),
            size_bytes: 10,
        };
        let metadata = KnowledgeMetadata {
            workspace_id: "scoped".into(),
            workspace_path: "/tmp/workspace".into(),
            collection_name: "kb_scoped".into(),
            created_at: now,
            last_updated: now,
            document_count: 2,
            chunk_count: 2,
            indexed_files: vec![
                shared("default", "default-id"),
                shared("research", "research-id"),
            ],
            members: vec!["shared.md".into()],
            collections: HashMap::from([
                ("default".into(), vec!["shared.md".into()]),
                ("research".into(), vec!["shared.md".into(), "new.md".into()]),
            ]),
            failures: Vec::new(),
            pending_retirements: Vec::new(),
        };
        let mut store = MetadataStore {
            dir: dir.clone(),
            metadata: Some(metadata),
        };
        let new_document = Document {
            id: "new-id".into(),
            path: "new.md".into(),
            title: "new".into(),
            content: "new content".into(),
            file_hash: "new".into(),
            collection: "research".into(),
            source_type: "markdown".into(),
            size_bytes: 11,
        };
        store
            .update(
                &[new_document],
                &HashMap::from([("new-id".into(), 1)]),
                1,
                &["shared.md".into()],
            )
            .unwrap();

        let indexed = &store.get_metadata().unwrap().indexed_files;
        assert!(indexed
            .iter()
            .any(|file| { file.collection == "default" && file.path == "shared.md" }));
        assert!(!indexed
            .iter()
            .any(|file| { file.collection == "research" && file.path == "shared.md" }));
        assert!(indexed
            .iter()
            .any(|file| { file.collection == "research" && file.path == "new.md" }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn atomic_metadata_write_replaces_complete_content() {
        let directory =
            std::env::temp_dir().join(format!("inkuo-metadata-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("metadata.json");
        std::fs::write(&destination, br#"{"version":"old"}"#).unwrap();

        atomic_write_metadata(&destination, br#"{"version":"new","complete":true}"#).unwrap();

        assert_eq!(
            std::fs::read(&destination).unwrap(),
            br#"{"version":"new","complete":true}"#
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn failed_metadata_activation_restores_previous_file() {
        let directory =
            std::env::temp_dir().join(format!("inkuo-metadata-restore-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("metadata.json");
        let missing_stage = directory.join("missing.tmp");
        std::fs::write(&destination, b"last-known-good").unwrap();

        replace_staged_metadata(&missing_stage, &destination)
            .expect_err("activation from a missing stage must fail");

        assert_eq!(std::fs::read(&destination).unwrap(), b"last-known-good");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn pending_retirements_survive_round_trip_until_explicitly_cleared() {
        let directory =
            std::env::temp_dir().join(format!("inkuo-metadata-retire-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let now = Utc::now();
        let mut store = MetadataStore {
            dir: directory.clone(),
            metadata: Some(KnowledgeMetadata {
                workspace_id: "retire".into(),
                workspace_path: "/tmp/workspace".into(),
                collection_name: "kb_retire".into(),
                created_at: now,
                last_updated: now,
                document_count: 0,
                chunk_count: 0,
                indexed_files: Vec::new(),
                members: Vec::new(),
                collections: HashMap::from([("research".into(), Vec::new())]),
                failures: Vec::new(),
                pending_retirements: vec![PendingRetirement::new(
                    "research",
                    "paper.pdf",
                    "old-generation",
                )],
            }),
        };
        store.save().unwrap();

        let mut reloaded = MetadataStore {
            dir: directory.clone(),
            metadata: None,
        };
        reloaded.load().unwrap();
        assert_eq!(
            reloaded.pending_retirements_for_collection("research")[0].document_id,
            "old-generation"
        );
        reloaded
            .clear_pending_retirements("research", &["old-generation".into()])
            .unwrap();
        assert!(reloaded
            .pending_retirements_for_collection("research")
            .is_empty());
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn loaded_collection_keys_are_canonicalized_without_cross_collection_leakage() {
        let now = Utc::now();
        let mut metadata = KnowledgeMetadata {
            workspace_id: "canonical".into(),
            workspace_path: "/tmp/workspace".into(),
            collection_name: "kb".into(),
            created_at: now,
            last_updated: now,
            document_count: 0,
            chunk_count: 0,
            indexed_files: Vec::new(),
            members: vec!["legacy-union.md".into()],
            collections: HashMap::from([
                (" default ".into(), vec!["a.md".into()]),
                ("Research\n Notes".into(), vec!["b.pdf".into()]),
            ]),
            failures: Vec::new(),
            pending_retirements: Vec::new(),
        };
        normalize_loaded_metadata(&mut metadata);
        assert_eq!(metadata.collections["default"], vec!["a.md"]);
        assert_eq!(metadata.collections["Research Notes"], vec!["b.pdf"]);
        assert!(!metadata.collections["default"].contains(&"b.pdf".to_string()));
    }
}
