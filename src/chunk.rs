pub fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![];
    }
    if max_chars == 0 {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;

    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        chunks.push(chunk);

        if end == chars.len() {
            break;
        }

        start += max_chars - overlap.min(max_chars - 1).max(1);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_is_one_chunk() {
        let text = "Hello world.";
        let chunks = chunk_text(text, 100, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn test_long_text_splits() {
        let text = "abcdefghij"; // 10 chars
        let chunks = chunk_text(text, 4, 2);
        // "abcd" (start 0, end 4)
        // start += 4 - 2 = 2
        // "cdef" (start 2, end 6)
        // start += 4 - 2 = 4
        // "efgh" (start 4, end 8)
        // start += 4 - 2 = 6
        // "ghij" (start 6, end 10)
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0], "abcd");
        assert_eq!(chunks[1], "cdef");
        assert_eq!(chunks[2], "efgh");
        assert_eq!(chunks[3], "ghij");
    }
}
