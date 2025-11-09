use faktory::{Job, WorkerBuilder};
use job_types::{JobPayload, MathArgs};
use std::io;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{error, info, warn};

type Result<T> = std::result::Result<T, io::Error>;

/// Handler for addition jobs
fn handle_add(args: MathArgs) -> Result<f64> {
    let result = args.a + args.b;
    // Logging removed for performance - in production you'd log selectively
    Ok(result)
}

/// Handler for subtraction jobs
fn handle_subtract(args: MathArgs) -> Result<f64> {
    let result = args.a - args.b;
    Ok(result)
}

/// Handler for multiplication jobs
fn handle_multiply(args: MathArgs) -> Result<f64> {
    let result = args.a * args.b;
    Ok(result)
}

/// Handler for division jobs
fn handle_divide(args: MathArgs) -> Result<f64> {
    if args.b == 0.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Division by zero",
        ));
    }
    let result = args.a / args.b;
    Ok(result)
}

/// Generic job handler that dispatches to specific handlers
async fn job_handler(job: Job) -> Result<()> {
    let job_type = job.kind();

    // Get the first argument (our job payload)
    let args_value = job
        .args()
        .get(0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Job missing arguments"))?
        .clone();

    // Parse into our typed JobPayload
    let payload = JobPayload::from_job_type(job_type, args_value).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Failed to parse job payload: {}", e),
        )
    })?;

    // Dispatch to the appropriate handler
    let result = match payload {
        JobPayload::Add(args) => handle_add(args),
        JobPayload::Subtract(args) => handle_subtract(args),
        JobPayload::Multiply(args) => handle_multiply(args),
        JobPayload::Divide(args) => handle_divide(args),
    };

    match result {
        Ok(_value) => {
            // Job completed successfully - only log errors in production
            Ok(())
        }
        Err(e) => {
            error!("Job failed: {:#}", e);
            Err(e)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Configuration
    let faktory_url =
        std::env::var("FAKTORY_URL").unwrap_or_else(|_| "tcp://localhost:7419".to_string());

    info!("Starting worker service");
    info!("Connecting to Faktory at: {}", faktory_url);

    // Set FAKTORY_URL environment variable for the client
    std::env::set_var("FAKTORY_URL", &faktory_url);

    // Setup graceful shutdown
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    // Handle SIGTERM and SIGINT for graceful shutdown
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to setup SIGTERM handler");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("Failed to setup SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                warn!("Received SIGTERM, initiating graceful shutdown...");
            }
            _ = sigint.recv() => {
                warn!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
            }
        }
        shutdown_clone.notify_one();
    });

    // Get worker concurrency from environment or use high default for network efficiency
    let worker_concurrency = std::env::var("WORKER_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500); // High concurrency to hide network latency

    // Build worker and register handlers with balanced concurrency
    let mut worker = WorkerBuilder::default()
        .hostname("worker-service".to_string())
        .workers(worker_concurrency) // High concurrency masks network fetch latency
        .register_fn("math_add", job_handler)
        .register_fn("math_subtract", job_handler)
        .register_fn("math_multiply", job_handler)
        .register_fn("math_divide", job_handler)
        .connect()
        .await?;

    info!("Worker connected and ready to process jobs");
    info!("Concurrency: {} jobs per worker", worker_concurrency);
    info!("Registered handlers: math_add, math_subtract, math_multiply, math_divide");

    // Run worker with graceful shutdown support
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.run(&["default"]).await {
            error!("Worker error: {:#}", e);
            Err::<(), _>(e)
        } else {
            Ok(())
        }
    });

    // Wait for shutdown signal
    shutdown.notified().await;
    info!("Shutdown signal received, stopping worker...");

    // Give worker time to finish current jobs (up to 30 seconds)
    tokio::select! {
        result = worker_handle => {
            match result {
                Ok(Ok(())) => info!("Worker shut down cleanly"),
                Ok(Err(e)) => error!("Worker error during shutdown: {:#}", e),
                Err(e) => error!("Worker task panicked: {:#}", e),
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
            warn!("Worker shutdown timeout after 30s, some jobs may be re-queued by Faktory");
        }
    }

    info!("Worker service terminated");
    Ok(())
}
