use anyhow::Result;
use chrono::Utc;
use slug::slugify;
use std::env;
use std::fs;
use std::path::PathBuf;

pub fn save_markdown(topic: &str, content: &str) -> Result<PathBuf> {
    let slug = slugify(topic);
    let kb_root = env::var("KB_ROOT").unwrap_or_else(|_| "kb".to_string());
    let dir = PathBuf::from(kb_root).join("topics");
    fs::create_dir_all(&dir)?;

    let file_path = dir.join(format!("{}.md", slug));
    let full_content = format!(
        "---
title: {}
slug: {}
created_at: {}
---

{}",
        topic,
        slug,
        Utc::now().to_rfc3339(),
        content
    );

    fs::write(&file_path, full_content)?;
    Ok(file_path)
}
