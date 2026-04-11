use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, ModelTrait, TextEmbedding};

pub const DIMS: usize = 384;

/// The HuggingFace model code for the active embedding model (e.g. "BAAI/bge-small-en-v1.5").
/// Derived from fastembed's default at runtime so it automatically reflects the library's
/// current default. On a major fastembed version bump: verify DIMS is still correct and
/// write a schema migration to resize F32_BLOB if the new default uses different dimensions.
pub fn model_id() -> String {
    let m = EmbeddingModel::default();
    EmbeddingModel::get_model_info(&m)
        .map(|info| info.model_code.clone())
        .unwrap_or_else(|| m.to_string())
}

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn load() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
            .join("memso")
            .join("models");

        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::default())
                .with_cache_dir(cache_dir)
                .with_show_download_progress(true),
        )
        .context("Failed to load embedding model")?;

        Ok(Self { model })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let results = self
            .model
            .embed(vec![text], None)
            .context("Embedding failed")?;
        results.into_iter().next().context("Embedding model returned no results")
    }
}

/// Encode a float slice as a little-endian byte blob for libsql vector storage.
/// Shared by store and retrieve to avoid duplication.
pub fn floats_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_to_blob_roundtrip() {
        let floats = vec![1.0f32, 2.0f32, -3.5f32];
        let blob = floats_to_blob(&floats);
        assert_eq!(blob.len(), 12); // 3 floats * 4 bytes each
        let decoded: Vec<f32> = blob
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(decoded, floats);
    }
}
