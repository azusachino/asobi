use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct DigestOutput {
    pub session_summary: String,
    pub topics: Vec<DigestedTopic>,
}

#[derive(Debug, Deserialize)]
pub struct DigestedTopic {
    pub title: String,
    pub content: String,
}

pub fn write_session_file(topics_root: &str, summary: &str) -> Result<PathBuf> {
    let sessions_dir = PathBuf::from(topics_root).join("sessions");
    fs::create_dir_all(&sessions_dir)?;
    let id = Utc::now().format("%Y-%m-%d-%H%M").to_string();
    let path = sessions_dir.join(format!("{}.md", id));
    let content = format!(
        "---
id: {}
created_at: {}
---

{}",
        id,
        Utc::now().to_rfc3339(),
        summary
    );
    fs::write(&path, content)?;
    Ok(path)
}

pub async fn call_digest_llm(transcript: &str) -> Result<DigestOutput> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
    let model =
        std::env::var("ROSEMARY_DIGEST_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let prompt = format!(
        r#"You are a memory assistant. Given the following conversation transcript, extract:
1. A concise session summary (2-3 sentences)
2. A list of distinct technical topics discussed, each with a title and a thorough markdown summary

Respond with valid JSON matching this schema:
{{
  "session_summary": "...",
  "topics": [
    {{"title": "...", "content": "..."}}
  ]
}}

Transcript:
---
{}
---"#,
        transcript
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": prompt}]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let json: serde_json::Value = resp.json().await?;
    let text = json["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("unexpected API response shape"))?;

    // Strip markdown code fences if present
    let text = text
        .trim()
        .trim_start_matches("```json")
        .trim_end_matches("```")
        .trim();
    Ok(serde_json::from_str(text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_session_file() {
        let dir = tempdir().unwrap();
        let path = write_session_file(dir.path().to_str().unwrap(), "Test summary").unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Test summary"));
    }
}
