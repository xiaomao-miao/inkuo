//! Text chunking module - splits documents into manageable chunks

use crate::knowledge::config::Chunk;
use regex::Regex;
use std::sync::LazyLock;

/// Regex for sentence boundary detection
static SENTENCE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[。！？\.!\?]+").expect("Invalid regex")
});

/// Regex for Chinese sentence boundary
static CHINESE_SENTENCE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[。！？\.\!\?]").expect("Invalid regex")
});

/// Chunking configuration
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Target chunk size in characters
    pub target_size: usize,
    /// Chunk overlap in characters
    pub overlap: usize,
    /// Minimum chunk size (discard smaller chunks)
    pub min_size: usize,
    /// Whether to preserve header boundaries
    pub preserve_headers: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_size: 500,
            overlap: 50,
            min_size: 50,
            preserve_headers: true,
        }
    }
}

/// Text chunker
pub struct Chunker {
    config: ChunkConfig,
}

impl Chunker {
    pub fn new(config: ChunkConfig) -> Self {
        Self { config }
    }

    /// Chunk a single document
    pub fn chunk_document(&self, doc_id: &str, doc_title: &str, content: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut char_index = 0;

        // Clean content
        let content = content.trim();
        let total_chars = content.chars().count();

        while char_index < total_chars {
            // Find the end position in characters
            let char_end = (char_index + self.config.target_size).min(total_chars);

            // Try to find a sentence boundary in characters
            let actual_char_end = if char_end < total_chars {
                self.find_sentence_boundary_char(&content, char_index, char_end)
                    .unwrap_or(char_end)
            } else {
                char_end
            };

            // Extract by character index, convert to byte offset for slicing
            let byte_start = content
                .chars()
                .take(char_index)
                .map(|c| c.len_utf8())
                .sum();
            let byte_end = content
                .chars()
                .take(actual_char_end)
                .map(|c| c.len_utf8())
                .sum();

            // Defensively check char boundary; if corrupted, skip to next safe point
            let chunk_text = if byte_start < content.len() && byte_end <= content.len() {
                content[byte_start..byte_end].trim().to_string()
            } else {
                String::new()
            };

            // Only keep chunks that meet minimum size
            if chunk_text.len() >= self.config.min_size {
                let chunk_start_line = line_number_for_byte_offset(content, byte_start);
                let chunk_end_line = line_number_for_byte_offset(content, byte_end.saturating_sub(1));
                let chunk = Chunk {
                    id: format!("{}_{}_{}", doc_id, doc_title, chunks.len()),
                    document_id: doc_id.to_string(),
                    content: chunk_text,
                    chunk_index: chunks.len(),
                    start_line: chunk_start_line,
                    end_line: chunk_end_line.max(chunk_start_line),
                    embedding: Vec::new(), // Will be filled later
                };
                chunks.push(chunk);
            }

            // Move to next chunk position with overlap (in characters)
            if actual_char_end >= total_chars {
                break;
            }
            let next_char = (actual_char_end as isize - self.config.overlap as isize)
                .max(char_index as isize) as usize;
            if next_char <= char_index {
                // Safety: advance at least 1 character to prevent infinite loop
                char_index += 1;
            } else {
                char_index = next_char;
            }
        }

        chunks
    }

    /// Find the best sentence boundary between char_index and char_end (both in characters).
    fn find_sentence_boundary_char(&self, content: &str, char_index: usize, char_end: usize) -> Option<usize> {
        // Convert char range to byte range for regex searching
        let byte_start: usize = content.chars().take(char_index).map(|c| c.len_utf8()).sum();
        let byte_end: usize = content.chars().take(char_end).map(|c| c.len_utf8()).sum();
        let slice = &content[byte_start..byte_end];

        // Try Chinese punctuation near the end of the window
        if let Some(m) = CHINESE_SENTENCE_REGEX.find_iter(slice).last() {
            return Some(char_index + slice[..m.end()].chars().count());
        }

        // Try English punctuation near the end of the window
        if let Some(m) = SENTENCE_REGEX.find_iter(slice).last() {
            return Some(char_index + slice[..m.end()].chars().count());
        }

        // Try line boundary
        if let Some(pos) = slice.rmatch_indices('\n').next() {
            let line_char_count = slice[pos.0..].chars().count();
            if line_char_count > 10 {
                return Some(char_end - line_char_count + slice[pos.0..].trim_end().chars().count());
            }
        }

        // Try paragraph boundary (double newline)
        if let Some(pos) = slice.rmatch_indices("\n\n").next() {
            let para_char_count = slice[pos.0..].chars().count();
            if para_char_count > 10 {
                return Some(char_end - para_char_count + slice[pos.0..].trim_end().chars().count());
            }
        }

        // Try word boundary (whitespace)
        if let Some(pos) = slice.rmatch_indices(|c: char| c.is_whitespace()).next() {
            let word_char_count = slice[pos.0..].chars().count();
            if word_char_count > 10 {
                return Some(char_end - word_char_count + slice[pos.0..].trim_end().chars().count());
            }
        }

        None
    }

    /// Chunk multiple documents
    pub fn chunk_documents(&self, documents: &[crate::knowledge::config::Document]) -> Vec<Chunk> {
        let mut all_chunks = Vec::new();

        for doc in documents {
            let doc_chunks = self.chunk_document(&doc.id, &doc.title, &doc.content);
            all_chunks.extend(doc_chunks);
        }

        tracing::info!("Chunked {} documents into {} chunks", documents.len(), all_chunks.len());

        all_chunks
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(ChunkConfig::default())
    }
}

fn line_number_for_byte_offset(content: &str, byte_offset: usize) -> usize {
    if content.is_empty() {
        return 1;
    }

    let safe_offset = byte_offset.min(content.len());
    let mut line = 1;

    for (idx, ch) in content.char_indices() {
        if idx >= safe_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunker_basic() {
        let chunker = Chunker::default();
        let content = "这是第一段内容。这是第二段内容。这是第三段内容。".repeat(50);
        let chunks = chunker.chunk_document("doc1", "Test", &content);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.content.len() <= 600); // target_size + some margin
        }
    }
}
