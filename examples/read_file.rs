use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use tokio::sync::watch;
use tokio::time::sleep;
use tokio::{fs::File as AsyncFile, io::AsyncReadExt};

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, mut rx) = watch::channel(false);
    tokio::spawn(watch_file_changes(tx));
    loop {
        let _ = rx.changed().await;
        if let Ok(contents) = read_file("data.txt").await {
            println!("{}", contents);
        }
    }
}

async fn read_file(filename: &str) -> Result<String> {
    let mut file = AsyncFile::open(filename).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    Ok(content)
}

async fn watch_file_changes(tx: watch::Sender<bool>) -> Result<()> {
    let path = PathBuf::from("data.txt");
    let mut last_modified = None;
    loop {
        if let Ok(metadata) = path.metadata() {
            let modified = metadata.modified()?;
            if last_modified != Some(modified) {
                last_modified = Some(modified);
                let _ = tx.send(true);
            }
        }
        sleep(Duration::from_millis(300)).await;
    }
}
