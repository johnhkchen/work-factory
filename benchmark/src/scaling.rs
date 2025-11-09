use anyhow::Result;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn get_queue_size() -> Result<u64> {
    let output = Command::new("curl")
        .args(&["-s", "http://localhost:7420/"])
        .output()?;

    let html = String::from_utf8_lossy(&output.stdout);

    // Extract enqueued count (4th occurrence of count)
    let mut count = 0;
    for line in html.lines() {
        if line.contains("class=\"count\"") {
            count += 1;
            if count == 4 {
                if let Some(start) = line.find('>') {
                    if let Some(end) = line[start..].find('<') {
                        let num_str = &line[start + 1..start + end];
                        let cleaned = num_str.replace(",", "");
                        return Ok(cleaned.parse().unwrap_or(0));
                    }
                }
            }
        }
    }
    Ok(0)
}

fn enqueue_jobs(num_jobs: u64) -> Result<Duration> {
    println!("  Enqueuing {} jobs...", num_jobs);
    let start = Instant::now();

    let output = Command::new("cargo")
        .args(&[
            "run",
            "--release",
            "--bin",
            "large",
            "--manifest-path",
            "benchmark/Cargo.toml",
            "--",
            &num_jobs.to_string(),
        ])
        .current_dir("/Users/johnchen/Documents/swe/repos/work-factory")
        .output()?;

    if !output.status.success() {
        // Fallback: run default 500k
        Command::new("sh")
            .args(&[
                "-c",
                "cd /Users/johnchen/Documents/swe/repos/work-factory/benchmark && cargo run --release --bin large >/dev/null 2>&1",
            ])
            .output()?;
    }

    Ok(start.elapsed())
}

fn scale_workers(replicas: u32) -> Result<()> {
    println!("  Scaling to {} workers...", replicas);

    // Update docker-compose.yml
    let compose_path = "/Users/johnchen/Documents/swe/repos/work-factory/docker-compose.yml";
    let content = std::fs::read_to_string(compose_path)?;
    let updated = content
        .replace("replicas: 2", &format!("replicas: {}", replicas))
        .replace("replicas: 4", &format!("replicas: {}", replicas))
        .replace("replicas: 6", &format!("replicas: {}", replicas))
        .replace("replicas: 8", &format!("replicas: {}", replicas))
        .replace("replicas: 10", &format!("replicas: {}", replicas));

    std::fs::write(compose_path, updated)?;

    // Apply changes
    Command::new("docker")
        .args(&["compose", "up", "-d", "worker-service"])
        .current_dir("/Users/johnchen/Documents/swe/repos/work-factory")
        .output()?;

    sleep(Duration::from_secs(3));
    Ok(())
}

fn stop_workers() -> Result<()> {
    Command::new("docker")
        .args(&["compose", "stop", "worker-service"])
        .current_dir("/Users/johnchen/Documents/swe/repos/work-factory")
        .output()?;
    sleep(Duration::from_secs(2));
    Ok(())
}

fn measure_processing_rate(workers: u32, job_count: u64) -> Result<(f64, Duration)> {
    println!(
        "\n=== Testing {} workers with {} jobs ===",
        workers, job_count
    );

    // Stop workers and enqueue jobs
    stop_workers()?;
    enqueue_jobs(job_count)?;

    let initial_queue = get_queue_size()?;
    println!("  Queue size: {}", initial_queue);

    if initial_queue == 0 {
        println!("  WARNING: No jobs in queue, skipping test");
        return Ok((0.0, Duration::from_secs(0)));
    }

    // Scale and start workers
    scale_workers(workers)?;

    println!("  Measuring processing rate...");
    let start = Instant::now();
    let mut last_queue = initial_queue;
    let mut samples = Vec::new();

    // Sample every second for up to 60 seconds or until queue is empty
    for i in 1..=60 {
        sleep(Duration::from_secs(1));
        let current_queue = get_queue_size()?;
        let processed = last_queue.saturating_sub(current_queue);

        if processed > 0 {
            samples.push(processed as f64);
            print!(".");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }

        last_queue = current_queue;

        if current_queue == 0 {
            println!("\n  Queue drained in {} seconds", i);
            break;
        }
    }

    let elapsed = start.elapsed();
    let total_processed = initial_queue - get_queue_size()?;
    let avg_rate = total_processed as f64 / elapsed.as_secs_f64();

    // Calculate peak rate (max from samples)
    let peak_rate = samples.iter().cloned().fold(0.0f64, f64::max);

    println!("\n  Total processed: {}", total_processed);
    println!("  Time: {:.2}s", elapsed.as_secs_f64());
    println!("  Average rate: {:.0} jobs/sec", avg_rate);
    println!("  Peak rate: {:.0} jobs/sec", peak_rate);

    Ok((avg_rate, elapsed))
}

fn main() -> Result<()> {
    println!("=== Work Factory Scaling Benchmark ===");
    println!("Finding optimal worker-to-CPU ratio\n");
    println!("This will take approximately 20 minutes...\n");

    let test_configs = vec![
        (2, 500_000),  // 2 workers, 500k jobs
        (4, 500_000),  // 4 workers, 500k jobs
        (6, 500_000),  // 6 workers, 500k jobs
        (8, 500_000),  // 8 workers, 500k jobs
        (10, 500_000), // 10 workers, 500k jobs
    ];

    let mut results = Vec::new();

    for (workers, jobs) in test_configs {
        match measure_processing_rate(workers, jobs) {
            Ok((rate, duration)) => {
                results.push((workers, rate, duration));
            }
            Err(e) => {
                println!("  ERROR: {}", e);
            }
        }

        // Cool down between tests
        println!("\n  Cooling down for 30 seconds...\n");
        sleep(Duration::from_secs(30));
    }

    // Print summary
    println!("\n\n=== SCALING BENCHMARK RESULTS ===\n");
    println!(
        "{:<10} {:<15} {:<15} {:<15}",
        "Workers", "Avg Rate", "Time (s)", "Rate/Worker"
    );
    println!("{:-<55}", "");

    for (workers, rate, duration) in &results {
        let rate_per_worker = rate / *workers as f64;
        println!(
            "{:<10} {:<15.0} {:<15.2} {:<15.0}",
            workers,
            rate,
            duration.as_secs_f64(),
            rate_per_worker
        );
    }

    // Find sweet spot (best rate per worker)
    if let Some((best_workers, _, _)) = results.iter().max_by(|a, b| {
        let rate_a = a.1 / a.0 as f64;
        let rate_b = b.1 / b.0 as f64;
        rate_a.partial_cmp(&rate_b).unwrap()
    }) {
        println!("\n=== RECOMMENDATION ===");
        println!("Optimal configuration: {} workers", best_workers);
        println!("This provides the best rate-per-worker efficiency");
    }

    // Find peak throughput
    if let Some((peak_workers, peak_rate, _)) =
        results.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    {
        println!(
            "\nPeak throughput: {:.0} jobs/sec with {} workers",
            peak_rate, peak_workers
        );
    }

    Ok(())
}
