use anyhow::Result;
use std::time::Instant;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Work Factory Throughput Benchmark ===\n");

    let client = reqwest::Client::new();
    let api_url = "http://localhost:3000"; // Direct to API service

    // Test 1: Sequential baseline
    println!("Test 1: Sequential requests (baseline)");
    let start = Instant::now();
    for i in 0..100 {
        let _ = client
            .post(format!("{}/jobs/add", api_url))
            .json(&serde_json::json!({"a": i, "b": i}))
            .send()
            .await?;
    }
    let elapsed = start.elapsed();
    println!("  100 jobs in {:?}", elapsed);
    println!("  Throughput: {:.2} jobs/sec\n", 100.0 / elapsed.as_secs_f64());

    // Test 2: Concurrent requests (10 at a time)
    println!("Test 2: Concurrent requests (10 concurrent)");
    let start = Instant::now();
    let mut set = JoinSet::new();
    for i in 0..100 {
        let client = client.clone();
        let url = api_url.to_string();
        set.spawn(async move {
            client
                .post(format!("{}/jobs/add", url))
                .json(&serde_json::json!({"a": i, "b": i}))
                .send()
                .await
        });

        // Limit concurrency
        if set.len() >= 10 {
            set.join_next().await;
        }
    }
    while set.join_next().await.is_some() {}
    let elapsed = start.elapsed();
    println!("  100 jobs in {:?}", elapsed);
    println!("  Throughput: {:.2} jobs/sec\n", 100.0 / elapsed.as_secs_f64());

    // Test 3: High concurrency (50 at a time)
    println!("Test 3: High concurrency (50 concurrent)");
    let start = Instant::now();
    let mut set = JoinSet::new();
    for i in 0..1000 {
        let client = client.clone();
        let url = api_url.to_string();
        set.spawn(async move {
            client
                .post(format!("{}/jobs/add", url))
                .json(&serde_json::json!({"a": i, "b": i}))
                .send()
                .await
        });

        if set.len() >= 50 {
            set.join_next().await;
        }
    }
    while set.join_next().await.is_some() {}
    let elapsed = start.elapsed();
    println!("  1000 jobs in {:?}", elapsed);
    println!("  Throughput: {:.2} jobs/sec\n", 1000.0 / elapsed.as_secs_f64());

    // Test 4: Maximum throughput (100 concurrent)
    println!("Test 4: Maximum throughput (100 concurrent)");
    let start = Instant::now();
    let mut set = JoinSet::new();
    for i in 0..5000 {
        let client = client.clone();
        let url = api_url.to_string();
        set.spawn(async move {
            client
                .post(format!("{}/jobs/add", url))
                .json(&serde_json::json!({"a": i, "b": i}))
                .send()
                .await
        });

        if set.len() >= 100 {
            set.join_next().await;
        }
    }
    while set.join_next().await.is_some() {}
    let elapsed = start.elapsed();
    println!("  5000 jobs in {:?}", elapsed);
    println!("  Throughput: {:.2} jobs/sec\n", 5000.0 / elapsed.as_secs_f64());

    println!("Benchmark complete!");
    Ok(())
}
