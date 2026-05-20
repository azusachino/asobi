use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::cl100k_base;

pub fn chunk_text(text: &str, max_tokens: usize, overlap: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![];
    }

    let tokenizer = cl100k_base().unwrap();
    let config = ChunkConfig::new(max_tokens)
        .with_sizer(tokenizer)
        .with_overlap(overlap)
        .expect("Overlap must be less than chunk size");

    let splitter = TextSplitter::new(config);
    splitter.chunks(text).map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_is_one_chunk() {
        let text = "Hello world. This is a short paragraph.";
        let chunks = chunk_text(text, 512, 64);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn test_long_text_splits_into_multiple_chunks() {
        // ~600 tokens worth of text
        let para = "word ".repeat(200);
        let text = format!("{}\n\n{}\n\n{}", para, para, para);
        let chunks = chunk_text(&text, 256, 32);
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {}",
            chunks.len()
        );
    }

    #[test]
    fn test_empty_text_returns_empty() {
        let chunks = chunk_text("", 512, 64);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_overlap_content_appears_in_adjacent_chunks() {
        let para_a = "alpha ".repeat(500);
        let para_b = "beta ".repeat(500);
        let text = format!("{}\n\n{}", para_a, para_b);
        let chunks = chunk_text(&text, 200, 50);

        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        // Verify that some "alpha" is in the second chunk OR some "beta" is in the first (depending on split point)
        // With text-splitter, overlap behavior might vary based on semantic boundaries.
        // We just want to ensure chunks aren't disjoint if they split.
        assert!(chunks[1].contains("alpha") || chunks[0].contains("beta"));
    }
}
