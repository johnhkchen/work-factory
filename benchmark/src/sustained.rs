use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Work Factory Sustained Throughput Benchmark ===\n");
    println!("This test measures sustained job processing rate over time.");
    println!("Jobs are enqueued continuously while workers process them.\n");

    let client = reqwest::Client::new();
    let api_url = "http://localhost:3000";

    // Sustained throughput test - enqueue jobs for 60 seconds
    println!("Test: Sustained load (60 seconds, 50 concurrent producers)");
    println!("Starting...\n");

    let start = Instant::now();
    let test_duration = Duration::from_secs(60);
    let mut total_enqueued = 0u64;
    let mut job_counter = 0u64;

    while start.elapsed() < test_duration {
        let mut set = JoinSet::new();

        // Enqueue 1000 jobs in batches of 50 concurrent
        for _ in 0..1000 {
            let client = client.clone();
            let url = api_url.to_string();
            let job_id = job_counter;
            job_counter += 1;

            set.spawn(async move {
                client
                    .post(format!("{}/jobs/add", url))
                    .json(&serde_json::json!({"a": job_id, "b": job_id}))
                    .send()
                    .await
            });

            if set.len() >= 50 {
                set.join_next().await;
            }
        }

        // Wait for remaining jobs in this batch
        while set.join_next().await.is_some() {}

        total_enqueued += 1000;

        let elapsed = start.elapsed();
        let rate = total_enqueued as f64 / elapsed.as_secs_f64();

        println!(
            "[{:>5.1}s] Enqueued: {:>6} jobs | Rate: {:>8.1} jobs/sec",
            elapsed.as_secs_f64(),
            total_enqueued,
            rate
        );
    }

    let total_time = start.elapsed();
    let avg_enqueue_rate = total_enqueued as f64 / total_time.as_secs_f64();

    println!("\n=== Enqueue Summary ===");
    println!("Total enqueued: {} jobs", total_enqueued);
    println!("Total time: {:.1}s", total_time.as_secs_f64());
    println!("Average enqueue rate: {:.1} jobs/sec", avg_enqueue_rate);

    println!("\n=== Waiting for Workers ===");
    println!("Waiting 30 seconds for workers to process the queue...");

    // Wait for workers to process
    tokio::time::sleep(Duration::from_secs(30)).await;

    // Check Faktory queue status
    println!("\nCheck http://localhost:7420 to see remaining queue depth");
    println!("Worker processing rate = (enqueued - remaining) / total_time");

    Ok(())
}
