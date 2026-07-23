//! Embedding module - generates embeddings using fastembed (ONNX-based local model)
//!
//! Supports two loading strategies:
//! - Native fastembed models (small, large): downloaded/loaded from cache
//! - User-defined models (base): loaded from locally bundled ONNX + tokenizer files

use crate::knowledge::config::Chunk;
use std::cmp::min;
use fastembed::{
    EmbeddingModel, InitOptions, InitOptionsUserDefined, Pooling, TextEmbedding,
    TokenizerFiles, UserDefinedEmbeddingModel,
};
use std::path::Path;

/// Embedding generation error
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("Failed to initialize model: {0}")]
    ModelInit(String),
    #[error("Failed to generate embedding: {0}")]
    Generation(String),
    #[error("Model not found at path: {0}")]
    ModelNotFound(String),
    #[error("Unsupported model: {0}")]
    UnsupportedModel(String),
}

/// Strategy for loading a model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// Built-in fastembed model (small, large)
    Native,
    /// User-defined model loaded from local ONNX + tokenizer files (base)
    UserDefined,
}

/// Model metadata
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub dimension: usize,
    pub source: ModelSource,
    /// Only set for native models
    pub fastembed_model: Option<EmbeddingModel>,
}

impl ModelInfo {
    pub fn new(model_name: &str) -> Result<Self, EmbedError> {
        match model_name {
            "BAAI/bge-small-zh-v1.5" => Ok(Self {
                dimension: 512,
                source: ModelSource::Native,
                fastembed_model: Some(EmbeddingModel::BGESmallZHV15),
            }),
            "BAAI/bge-base-zh-v1.5" => Ok(Self {
                dimension: 768,
                source: ModelSource::UserDefined,
                fastembed_model: None,
            }),
            "BAAI/bge-large-zh-v1.5" => Ok(Self {
                dimension: 1024,
                source: ModelSource::Native,
                fastembed_model: Some(EmbeddingModel::BGELargeZHV15),
            }),
            _ => Err(EmbedError::UnsupportedModel(model_name.to_string())),
        }
    }
}

/// Embedder for generating text embeddings using fastembed
pub struct Embedder {
    model: std::sync::Mutex<TextEmbedding>,
    dimension: usize,
    model_name: String,
}

impl Embedder {
    /// Create a new embedder from the specified model.
    ///
    /// model_path is the directory containing model files. Required even for native
    /// fastembed models (used as cache_dir for ONNX weights).
    pub fn new(model_name: &str, model_path: &Path) -> Result<Self, EmbedError> {
        let model_info = ModelInfo::new(model_name)?;

        if !model_path.exists() {
            return Err(EmbedError::ModelNotFound(format!(
                "Model path does not exist: {}",
                model_path.display()
            )));
        }

        tracing::info!(
            "Initializing embedder for '{}' from '{}' (source: {:?})",
            model_name,
            model_path.display(),
            model_info.source
        );

        // Reserve the fastembed variant ahead of the source `match` so the
        // Native branch never has to `unwrap()` an `Option`. Adding a new
        // model above means handling both fields together; the borrow
        // checker enforces this.
        let fastembed_model = model_info.fastembed_model;
        let model = match (model_info.source, fastembed_model) {
            (ModelSource::Native, Some(fastembed_model)) => {
                tracing::info!("[EMBEDDER] Loading native model, cache_dir: {:?}", model_path);
                let init_options = InitOptions::new(fastembed_model)
                    .with_cache_dir(model_path.to_path_buf())
                    .with_show_download_progress(false);
                tracing::info!("[EMBEDDER] Calling TextEmbedding::try_new (native)");
                TextEmbedding::try_new(init_options)
                    .map_err(|e| {
                        tracing::error!("[EMBEDDER] Native model init failed: {}", e);
                        EmbedError::ModelInit(format!("Native model init failed: {}", e))
                    })?
            }
            (ModelSource::UserDefined, None) => {
                tracing::info!("[EMBEDDER] Loading user-defined model");
                Self::load_user_defined_model(model_path, model_name)?
            }
            (ModelSource::UserDefined, Some(_)) => {
                return Err(EmbedError::ModelInit(
                    "Internal invariant violation: user-defined model has fastembed variant".to_string(),
                ));
            }
            (ModelSource::Native, None) => {
                return Err(EmbedError::ModelInit(
                    "Internal invariant violation: native model missing fastembed variant".to_string(),
                ));
            }
        };

        Ok(Self {
            model: std::sync::Mutex::new(model),
            dimension: model_info.dimension,
            model_name: model_name.to_string(),
        })
    }

// ── Model loading ───────────────────────────────────────────────────────────────

    fn load_user_defined_model(model_path: &Path, model_name: &str) -> Result<TextEmbedding, EmbedError> {
        let onnx_file = model_path.join("model.onnx");
        let tokenizer_json = model_path.join("tokenizer.json");
        let config_json = model_path.join("config.json");
        let special_tokens = model_path.join("special_tokens_map.json");
        let tokenizer_config = model_path.join("tokenizer_config.json");

        for (file, name) in [
            (&onnx_file, "model.onnx"),
            (&tokenizer_json, "tokenizer.json"),
            (&config_json, "config.json"),
            (&special_tokens, "special_tokens_map.json"),
            (&tokenizer_config, "tokenizer_config.json"),
        ] {
            if !file.exists() {
                return Err(EmbedError::ModelNotFound(format!(
                    "User-defined model '{}' is missing file: {}",
                    model_name, name
                )));
            }
        }

        let onnx_bytes = std::fs::read(&onnx_file)
            .map_err(|e| {
                tracing::error!("[EMBEDDER] Failed to read ONNX file {}: {}", onnx_file.display(), e);
                EmbedError::ModelInit(format!("Failed to read ONNX file: {}", e))
            })?;
        tracing::debug!("[EMBEDDER] ONNX file read, size: {} bytes", onnx_bytes.len());

        let tokenizer_files = TokenizerFiles {
            tokenizer_file: std::fs::read(&tokenizer_json)
                .map_err(|e| EmbedError::ModelInit(format!("Failed to read tokenizer.json: {}", e)))?,
            config_file: std::fs::read(&config_json)
                .map_err(|e| EmbedError::ModelInit(format!("Failed to read config.json: {}", e)))?,
            special_tokens_map_file: std::fs::read(&special_tokens)
                .map_err(|e| EmbedError::ModelInit(format!("Failed to read special_tokens_map.json: {}", e)))?,
            tokenizer_config_file: std::fs::read(&tokenizer_config)
                .map_err(|e| EmbedError::ModelInit(format!("Failed to read tokenizer_config.json: {}", e)))?,
        };

        tracing::debug!("[EMBEDDER] Tokenizer files loaded");

        let user_model = UserDefinedEmbeddingModel::new(onnx_bytes, tokenizer_files)
            .with_pooling(Pooling::Mean);

        tracing::info!("[EMBEDDER] Calling TextEmbedding::try_new_from_user_defined");
        TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::default())
            .map_err(|e| {
                tracing::error!("[EMBEDDER] try_new_from_user_defined failed: {}", e);
                EmbedError::ModelInit(format!("User-defined model init failed: {}", e))
            })
    }

    /// Generate embeddings for multiple texts

    // ── Encoding API ─────────────────────────────────────────────────────────────

    pub fn encode(&self, texts: Vec<&str>) -> Result<Vec<Vec<f32>>, EmbedError> {
        tracing::debug!("[EMBEDDER] encode() called with {} texts", texts.len());
        let mut model = self.model.lock().map_err(|e| EmbedError::Generation(e.to_string()))?;

        tracing::debug!("[EMBEDDER] Calling fastembed embed()");
        let embeddings = model
            .embed(texts, None)
            .map_err(|e| {
                tracing::error!("[EMBEDDER] embed() failed: {}", e);
                EmbedError::Generation(e.to_string())
            })?;
        tracing::debug!("[EMBEDDER] embed() returned {} embeddings", embeddings.len());

        Ok(embeddings.into_iter().map(|e| e.into()).collect())
    }

    /// Generate embedding for a single text
    pub fn encode_single(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        tracing::debug!("[EMBEDDER] encode_single() called, text length: {}", text.len());
        let embeddings = self.encode(vec![text])?;
        Ok(embeddings
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0f32; self.dimension]))
    }

    /// Generate embeddings for chunks in batches (fills in the embedding field)
    pub fn encode_chunks_batched<F>(
        &self,
        chunks: &mut [Chunk],
        batch_size: usize,
        mut on_batch_complete: F,
    ) -> Result<(), EmbedError>
    where
        F: FnMut(usize, usize),
    {
        let total_chunks = chunks.len();
        tracing::debug!(
            "[EMBEDDER] encode_chunks_batched() called with {} chunks, batch_size={}",
            total_chunks,
            batch_size
        );

        if total_chunks == 0 {
            return Ok(());
        }

        let safe_batch_size = batch_size.max(1);

        for batch_start in (0..total_chunks).step_by(safe_batch_size) {
            let batch_end = min(batch_start + safe_batch_size, total_chunks);
            let batch_index = batch_start / safe_batch_size + 1;
            let total_batches = total_chunks.div_ceil(safe_batch_size);
            let batch = &mut chunks[batch_start..batch_end];
            let texts: Vec<&str> = batch.iter().map(|c| c.content.as_str()).collect();
            let first_preview = texts.first().map(|t| {
                let preview: String = t.chars().take(50).collect();
                if t.chars().count() > 50 {
                    format!("{}...", preview)
                } else {
                    preview
                }
            });

            tracing::info!(
                "[EMBEDDER] Processing batch {}/{} (chunks {}-{} of {})",
                batch_index,
                total_batches,
                batch_start + 1,
                batch_end,
                total_chunks
            );
            tracing::debug!(
                "[EMBEDDER] Batch size: {}, first preview: {:?}",
                texts.len(),
                first_preview
            );

            let embeddings = self.encode(texts)?;

            for (chunk, embedding) in batch.iter_mut().zip(embeddings.into_iter()) {
                chunk.embedding = embedding;
            }

            on_batch_complete(batch_end, total_chunks);
            tracing::debug!(
                "[EMBEDDER] Batch {}/{} complete ({}/{})",
                batch_index,
                total_batches,
                batch_end,
                total_chunks
            );
        }

        tracing::debug!("[EMBEDDER] encode_chunks_batched() complete");
        Ok(())
    }

    /// Generate embeddings for chunks (fills in the embedding field)
    pub fn encode_chunks(&self, chunks: &mut [Chunk]) -> Result<(), EmbedError> {
        self.encode_chunks_batched(chunks, 64, |_, _| {})
    }

    /// Async-friendly wrapper around [`encode_chunks_batched`]. The underlying
    /// fastembed inference is CPU-bound and can take seconds per batch, so we
    /// must not run it on the tokio executor directly — it would block all
    /// IPC, AI chat, and other tasks sharing the worker thread.
    ///
    /// `spawn_blocking` would be the obvious tool, but `Embedder` is not
    /// `Send` (the underlying `fastembed::TextEmbedding` holds a non-`Send`
    /// ONNX session), and `spawn_blocking` requires `Send + 'static`.
    /// `tokio::task::block_in_place` is the right escape hatch: it temporarily
    /// converts the current worker thread into a blocking thread without
    /// requiring `Send`. Internally tokio hands other tasks off to another
    /// worker, so the executor as a whole keeps making progress.
    ///
    /// Progress reporting is intentionally absent here — the underlying
    /// `encode_chunks_batched` callback runs synchronously, and bridging it
    /// through an async channel would force the callback to be `Send +
    /// 'static`, which captures like `&AppHandle` cannot satisfy. Callers
    /// that need mid-pass progress should drive their own channel on top of
    /// the synchronous API.
    pub async fn encode_chunks_batched_async(
        &self,
        chunks: &mut Vec<Chunk>,
        batch_size: usize,
    ) -> Result<(), EmbedError> {
        // Move the chunk buffer into the blocking section via `mem::take`.
        // If `block_in_place` panics, the caller still owns an empty Vec
        // (via the original `*chunks` reference we restore at the end) and
        // can retry. `block_in_place` reborrows `&self` only for the
        // duration of the closure; the original lifetime of `&self` is held
        // by the outer async function so the borrow is sound.
        //
        // We catch panics (instead of letting them unwind across `await`
        // points, which would corrupt the buffer) and translate them into a
        // typed error. The most likely panic source is running this on a
        // single-threaded tokio runtime (e.g. `tokio::runtime::Builder::new_current_thread`),
        // which `block_in_place` rejects.
        let mut owned: Vec<Chunk> = std::mem::take(chunks);
        let self_ptr = self as *const Self;
        let result = tokio::task::block_in_place(|| {
            // Safety: `self_ptr` is the address of a borrow that the outer
            // async fn holds across the await; block_in_place runs on the
            // same thread, so no other task can mutate `*self` here.
            let embedder = unsafe { &*self_ptr };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                embedder.encode_chunks_batched(&mut owned, batch_size, |_completed, _total| {})
            })) {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(payload) => {
                    let message = if let Some(s) = payload.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else if let Some(s) = payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        // `block_in_place` specifically rejects call from a
                        // current-thread runtime with the message
                        // "Cannot block the current thread from within a
                        //  runtime". Detect that and surface a friendlier
                        // hint to the caller.
                        "blocking task panicked (likely single-threaded runtime)".to_string()
                    };
                    Err(EmbedError::Generation(format!(
                        "inference panicked: {} — multi-threaded tokio runtime required",
                        message
                    )))
                }
            }
        });
        *chunks = owned;
        result
    }

    /// Get embedding dimension
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}
