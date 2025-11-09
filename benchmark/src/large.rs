use anyhow::Result;
use std::time::Instant;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Large Queue Benchmark ===\n");
    println!("Enqueuing 2,000,000 jobs as fast as possible...\n");

    let client = reqwest::Client::new();
    let api_url = "http://localhost:3000";

    let start = Instant::now();
    let total_jobs = 2_000_000u64;
    let concurrency = 100;

    let mut job_counter = 0u64;

    while job_counter < total_jobs {
        let mut set = JoinSet::new();

        let batch_size = std::cmp::min(10_000, total_jobs - job_counter);

        for i in 0..batch_size {
            let client = client.clone();
            let url = api_url.to_string();
            let job_id = job_counter + i;

            set.spawn(async move {
                client
                    .post(format!("{}/jobs/add", url))
                    .json(&serde_json::json!({"a": job_id, "b": job_id}))
                    .send()
                    .await
            });

            if set.len() >= concurrency {
                set.join_next().await;
            }
        }

        while set.join_next().await.is_some() {}

        job_counter += batch_size;

        if job_counter % 50_000 == 0 {
            let elapsed = start.elapsed();
            let rate = job_counter as f64 / elapsed.as_secs_f64();
            println!("Enqueued {} jobs in {:.2}s ({:.0} jobs/sec)",
                     job_counter, elapsed.as_secs_f64(), rate);
        }
    }

    let total_time = start.elapsed();
    let avg_rate = total_jobs as f64 / total_time.as_secs_f64();

    println!("\n=== Summary ===");
    println!("Total enqueued: {} jobs", total_jobs);
    println!("Total time: {:.2}s", total_time.as_secs_f64());
    println!("Average rate: {:.0} jobs/sec", avg_rate);

    Ok(())
}
