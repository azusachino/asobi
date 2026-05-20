use anyhow::Result;
use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

use super::EmbeddingProvider;

pub struct FastEmbedProvider {
    model: Mutex<TextEmbedding>,
    dim: usize,
}

impl FastEmbedProvider {
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))?;
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
        let p = FastEmbedProvider::new().unwrap();
        assert_eq!(p.dim(), 384);
        let result = p.embed(&["hello".to_string()]).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 384);
    }
}
