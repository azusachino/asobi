use anyhow::Result;
use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::PathBuf;
use std::sync::Mutex;

use super::EmbeddingProvider;

pub struct FastEmbedProvider {
    model: Mutex<TextEmbedding>,
    dim: usize,
}

impl FastEmbedProvider {
    /// `cache_dir` is where fastembed stores downloaded model weights.
    /// Routing this through our workspace prevents the default
    /// `./.fastembed_cache` from being created in whichever directory the
    /// command happens to be invoked from.
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        let opts = InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_cache_dir(cache_dir);
        let model = TextEmbedding::try_new(opts)?;
        Ok(Self {
            model: Mutex::new(model),
            dim: 384,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for FastEmbedProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut model = self
            .model
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex poisoned: {}", e))?;
        let embeddings = model.embed(texts, None)?;
        Ok(embeddings)
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fastembed_provider() {
        let dir = tempfile::tempdir().unwrap();
        let p = FastEmbedProvider::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(p.dim(), 384);
        let result = p.embed(&["hello".to_string()]).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 384);
    }
}
