use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub const DIMS: usize = 384;

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
            InitOptions::new(EmbeddingModel::BGESmallENV15)
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

/// Encode a float slice as a JSON array string for turso vector32() input.
/// turso's vector32() function accepts '[1.0, 2.0, ...]' and encodes it
/// internally as a float32 blob.
pub fn floats_to_json(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8 + 2);
    s.push('[');
    for (i, f) in v.iter().enumerate() {
        if i > 0 { s.push(','); }
        s.push_str(&f.to_string());
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_to_json_format() {
        let floats = vec![1.0f32, 2.0f32, -3.5f32];
        let json = floats_to_json(&floats);
        assert_eq!(json, "[1,2,-3.5]");
    }
}
