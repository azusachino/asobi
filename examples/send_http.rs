use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use serde::Deserialize;
use tokio::time::Instant;
use tokio::time::sleep;

#[allow(unused)]
#[derive(Deserialize, Debug)]
struct Response {
    pub url: String,
    pub args: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let resp = reqwest::get("https://httpbin.org/ip")
        .await?
        .json::<HashMap<String, String>>()
        .await?;
    println!("{resp:#?}");

    let start_time = Instant::now();
    let data = fetch_data(5);
    let time_since = calculate_last_login();
    let (posts, _) = tokio::join!(data, time_since);
    let duration = start_time.elapsed();
    println!("fetched: {:?}, cost: {:?}", posts, duration);
    Ok(())
}

async fn fetch_data(seconds: u64) -> Result<Response> {
    let request_url = format!("https://httpbin.org/delay/{}", seconds);
    let r = reqwest::get(&request_url).await?;
    let dr: Response = r.json().await?;
    Ok(dr)
}

async fn calculate_last_login() {
    sleep(Duration::from_secs(1)).await;
    println!("logged in 2 days ago")
}
