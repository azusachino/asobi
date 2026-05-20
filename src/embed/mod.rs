use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dim(&self) -> usize;
}

pub mod fastembed_provider;
pub use fastembed_provider::FastEmbedProvider;

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyProvider;

    #[async_trait]
    impl EmbeddingProvider for DummyProvider {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.0f32; 4]).collect())
        }
        fn dim(&self) -> usize {
            4
        }
    }

    #[tokio::test]
    async fn test_provider_trait() {
        let p = DummyProvider;
        let result = p.embed(&["hello".to_string()]).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 4);
    }
}
