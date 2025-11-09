use reqwest::Client;
use serde_json::json;
use std::time::Instant;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let api_url = "http://localhost:3000";

    println!("=== Horizontal Scaling Test ===\n");
    println!("This test enqueues jobs as fast as possible,");
    println!("then measures how long workers take to process them.\n");

    // Phase 1: Enqueue 100k jobs as fast as possible
    println!("Phase 1: Enqueueing 100,000 jobs...");
    let enqueue_start = Instant::now();

    let mut handles = vec![];
    for _ in 0..100 {
        let client = client.clone();
        let api_url = api_url.to_string();

        let handle = tokio::spawn(async move {
            for _ in 0..1000 {
                let _ = client
                    .post(format!("{}/jobs/add", api_url))
                    .json(&json!({"a": 1.0, "b": 1.0}))
                    .send()
                    .await;
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await?;
    }

    let enqueue_elapsed = enqueue_start.elapsed();
    println!("Enqueued 100,000 jobs in {:.2}s ({:.0} jobs/sec)\n",
        enqueue_elapsed.as_secs_f64(),
        100_000.0 / enqueue_elapsed.as_secs_f64()
    );

    // Phase 2: Wait for queue to drain
    println!("Phase 2: Waiting for workers to process all jobs...");
    let process_start = Instant::now();

    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check queue depth via Faktory dashboard
        let resp = client.get("http://localhost:7420").send().await?;
        let html = resp.text().await?;

        // Parse queue count from HTML
        if let Some(start) = html.find(r#"<span class="count">"#) {
            if let Some(end) = html[start + 20..].find("</span>") {
                let count_str = &html[start + 20..start + 20 + end];
                if let Ok(count) = count_str.parse::<u32>() {
                    if count == 0 {
                        break;
                    }
                    print!("\rQueue depth: {}    ", count);
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
            }
        }

        // Timeout after 2 minutes
        if process_start.elapsed().as_secs() > 120 {
            println!("\n\nTimeout after 2 minutes");
            break;
        }
    }

    let process_elapsed = process_start.elapsed();
    let processing_rate = 100_000.0 / process_elapsed.as_secs_f64();

    println!("\n\nProcessed 100,000 jobs in {:.2}s ({:.0} jobs/sec)",
        process_elapsed.as_secs_f64(),
        processing_rate
    );

    println!("\n=== Results ===");
    println!("Enqueue throughput: {:.0} jobs/sec", 100_000.0 / enqueue_elapsed.as_secs_f64());
    println!("Processing throughput: {:.0} jobs/sec", processing_rate);
    println!("Worker efficiency: {:.1}%",
        (processing_rate / (100_000.0 / enqueue_elapsed.as_secs_f64())) * 100.0
    );

    Ok(())
}
