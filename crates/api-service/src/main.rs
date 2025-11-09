use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use deadpool::managed::{Manager, Pool, RecycleResult};
use faktory::{Client, Job};
use job_types::{JobPayload, MathArgs};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{info, warn};

/// Connection pool manager for Faktory clients
struct FaktoryManager {
    faktory_url: String,
}

impl Manager for FaktoryManager {
    type Type = Client;
    type Error = faktory::Error;

    async fn create(&self) -> Result<Client, faktory::Error> {
        std::env::set_var("FAKTORY_URL", &self.faktory_url);
        Client::connect().await
    }

    async fn recycle(
        &self,
        _conn: &mut Client,
        _metrics: &deadpool::managed::Metrics,
    ) -> RecycleResult<faktory::Error> {
        // Faktory connections don't need special recycling
        Ok(())
    }
}

/// Configuration for batch processing
#[derive(Clone)]
struct BatchConfig {
    /// Maximum number of jobs to batch together
    max_batch_size: usize,
    /// Maximum time to wait before flushing a batch (milliseconds)
    max_batch_delay_ms: u64,
    /// Whether to enable auto-batching for individual job endpoints
    auto_batch_enabled: bool,
}

/// Batching queue for collecting jobs
struct BatchQueue {
    pending_jobs: Vec<JobPayload>,
    config: BatchConfig,
}

impl BatchQueue {
    fn new(config: BatchConfig) -> Self {
        Self {
            pending_jobs: Vec::with_capacity(config.max_batch_size),
            config,
        }
    }

    fn add(&mut self, job: JobPayload) {
        self.pending_jobs.push(job);
    }

    fn should_flush(&self) -> bool {
        self.pending_jobs.len() >= self.config.max_batch_size
    }

    fn flush(&mut self) -> Vec<JobPayload> {
        std::mem::replace(
            &mut self.pending_jobs,
            Vec::with_capacity(self.config.max_batch_size),
        )
    }

    fn len(&self) -> usize {
        self.pending_jobs.len()
    }
}

/// Shared application state
#[derive(Clone)]
struct AppState {
    faktory_pool: Pool<FaktoryManager>,
    batch_queue: Arc<Mutex<BatchQueue>>,
    batch_config: BatchConfig,
}

#[derive(Debug, Deserialize)]
struct MathRequest {
    a: f64,
    b: f64,
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct JobResponse {
    job_id: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// Batch job request containing multiple operations
#[derive(Debug, Deserialize)]
struct BatchJobRequest {
    jobs: Vec<JobPayload>,
}

/// Response for batch job submission
#[derive(Debug, Serialize)]
struct BatchJobResponse {
    job_ids: Vec<String>,
    message: String,
    total_enqueued: usize,
}

/// Helper to enqueue a job to Faktory
async fn enqueue_job(pool: Pool<FaktoryManager>, payload: JobPayload) -> Result<String> {
    // Create job
    let job_type = payload.job_type();
    let args = payload.to_args()?;

    let job = Job::new(job_type, vec![args]);
    let job_id = job.id().to_string();

    // Get a connection from the pool
    let mut client = pool
        .get()
        .await
        .context("Failed to get Faktory connection from pool")?;

    // Push to Faktory
    client.enqueue(job).await.context("Failed to enqueue job")?;

    info!("Enqueued job {} of type {}", job_id, job_type);

    Ok(job_id)
}

/// Helper to enqueue multiple jobs in a batch (much more efficient over network)
async fn enqueue_batch_jobs(
    pool: Pool<FaktoryManager>,
    payloads: Vec<JobPayload>,
) -> Result<Vec<String>> {
    if payloads.is_empty() {
        return Ok(vec![]);
    }

    // Get a single connection from the pool for all jobs
    let mut client = pool
        .get()
        .await
        .context("Failed to get Faktory connection from pool")?;

    let mut job_ids = Vec::with_capacity(payloads.len());

    // Create all jobs first
    let mut jobs = Vec::with_capacity(payloads.len());
    for payload in payloads {
        let job_type = payload.job_type();
        let args = payload.to_args()?;
        let job = Job::new(job_type, vec![args]);
        job_ids.push(job.id().to_string());
        jobs.push(job);
    }

    // Enqueue all jobs using a single connection
    for job in jobs {
        client
            .enqueue(job)
            .await
            .context("Failed to enqueue job in batch")?;
    }

    info!("Enqueued batch of {} jobs", job_ids.len());

    Ok(job_ids)
}

/// Helper to enqueue a job with auto-batching support
/// This collects jobs and flushes them when the batch is full
async fn enqueue_job_with_batching(state: &AppState, payload: JobPayload) -> Result<String> {
    // Create the job to get its ID
    let job_type = payload.job_type();
    let args = payload.to_args()?;
    let job = Job::new(job_type, vec![args]);
    let job_id = job.id().to_string();

    // Add to batch queue
    let should_flush = {
        let mut queue = state.batch_queue.lock().await;
        queue.add(payload);
        queue.should_flush()
    };

    // If batch is full, flush it immediately
    if should_flush {
        let jobs_to_flush = {
            let mut queue = state.batch_queue.lock().await;
            queue.flush()
        };

        info!(
            "Auto-flushing batch of {} jobs (batch full)",
            jobs_to_flush.len()
        );
        enqueue_batch_jobs(state.faktory_pool.clone(), jobs_to_flush).await?;
    }

    Ok(job_id)
}

/// POST /jobs/add - Add two numbers
async fn add_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MathRequest>,
) -> impl IntoResponse {
    let payload = JobPayload::Add(MathArgs {
        a: req.a,
        b: req.b,
        request_id: req.request_id,
    });

    let result = if state.batch_config.auto_batch_enabled {
        enqueue_job_with_batching(&state, payload).await
    } else {
        enqueue_job(state.faktory_pool.clone(), payload).await
    };

    match result {
        Ok(job_id) => {
            let response = JobResponse {
                job_id,
                message: format!("Job enqueued to add {} + {}", req.a, req.b),
            };
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to enqueue job: {:#}", e);
            let response = ErrorResponse {
                error: format!("Failed to enqueue job: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// POST /jobs/subtract - Subtract two numbers
async fn subtract_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MathRequest>,
) -> impl IntoResponse {
    let payload = JobPayload::Subtract(MathArgs {
        a: req.a,
        b: req.b,
        request_id: req.request_id,
    });

    let result = if state.batch_config.auto_batch_enabled {
        enqueue_job_with_batching(&state, payload).await
    } else {
        enqueue_job(state.faktory_pool.clone(), payload).await
    };

    match result {
        Ok(job_id) => {
            let response = JobResponse {
                job_id,
                message: format!("Job enqueued to subtract {} - {}", req.a, req.b),
            };
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to enqueue job: {:#}", e);
            let response = ErrorResponse {
                error: format!("Failed to enqueue job: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// POST /jobs/multiply - Multiply two numbers
async fn multiply_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MathRequest>,
) -> impl IntoResponse {
    let payload = JobPayload::Multiply(MathArgs {
        a: req.a,
        b: req.b,
        request_id: req.request_id,
    });

    let result = if state.batch_config.auto_batch_enabled {
        enqueue_job_with_batching(&state, payload).await
    } else {
        enqueue_job(state.faktory_pool.clone(), payload).await
    };

    match result {
        Ok(job_id) => {
            let response = JobResponse {
                job_id,
                message: format!("Job enqueued to multiply {} × {}", req.a, req.b),
            };
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to enqueue job: {:#}", e);
            let response = ErrorResponse {
                error: format!("Failed to enqueue job: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// POST /jobs/divide - Divide two numbers
async fn divide_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MathRequest>,
) -> impl IntoResponse {
    let payload = JobPayload::Divide(MathArgs {
        a: req.a,
        b: req.b,
        request_id: req.request_id,
    });

    let result = if state.batch_config.auto_batch_enabled {
        enqueue_job_with_batching(&state, payload).await
    } else {
        enqueue_job(state.faktory_pool.clone(), payload).await
    };

    match result {
        Ok(job_id) => {
            let response = JobResponse {
                job_id,
                message: format!("Job enqueued to divide {} ÷ {}", req.a, req.b),
            };
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to enqueue job: {:#}", e);
            let response = ErrorResponse {
                error: format!("Failed to enqueue job: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// POST /jobs/batch - Submit multiple jobs at once for optimal network performance
async fn batch_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchJobRequest>,
) -> impl IntoResponse {
    let job_count = req.jobs.len();

    if job_count == 0 {
        let response = ErrorResponse {
            error: "Batch request must contain at least one job".to_string(),
        };
        return (StatusCode::BAD_REQUEST, Json(response)).into_response();
    }

    match enqueue_batch_jobs(state.faktory_pool.clone(), req.jobs).await {
        Ok(job_ids) => {
            let response = BatchJobResponse {
                total_enqueued: job_ids.len(),
                job_ids,
                message: format!("Successfully enqueued {} jobs in batch", job_count),
            };
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to enqueue batch jobs: {:#}", e);
            let response = ErrorResponse {
                error: format!("Failed to enqueue batch jobs: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "api-service"
    }))
}

/// Background task that periodically flushes the batch queue
async fn batch_flusher(
    pool: Pool<FaktoryManager>,
    batch_queue: Arc<Mutex<BatchQueue>>,
    flush_interval_ms: u64,
) {
    let interval = Duration::from_millis(flush_interval_ms);

    loop {
        sleep(interval).await;

        // Check if there are jobs to flush
        let jobs_to_flush = {
            let mut queue = batch_queue.lock().await;
            if queue.len() > 0 {
                Some(queue.flush())
            } else {
                None
            }
        };

        // Flush jobs if any
        if let Some(jobs) = jobs_to_flush {
            info!("Batch flusher: flushing {} jobs after timeout", jobs.len());
            if let Err(e) = enqueue_batch_jobs(pool.clone(), jobs).await {
                warn!("Batch flusher: failed to flush jobs: {:#}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Configuration
    let faktory_url =
        std::env::var("FAKTORY_URL").unwrap_or_else(|_| "tcp://localhost:7419".to_string());
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    // Batch configuration
    let max_batch_size = std::env::var("BATCH_MAX_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let max_batch_delay_ms = std::env::var("BATCH_MAX_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let auto_batch_enabled = std::env::var("BATCH_AUTO_ENABLED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);

    info!("Faktory URL: {}", faktory_url);
    info!("Binding to: {}", bind_addr);
    info!(
        "Batch config: max_size={}, max_delay={}ms, auto_batch={}",
        max_batch_size, max_batch_delay_ms, auto_batch_enabled
    );

    // Create Faktory connection pool
    let manager = FaktoryManager {
        faktory_url: faktory_url.clone(),
    };
    let faktory_pool = Pool::builder(manager)
        .max_size(50) // Allow up to 50 concurrent connections
        .build()
        .context("Failed to create Faktory connection pool")?;

    info!("Created Faktory connection pool with max size 50");

    // Test the pool by getting a connection
    info!("Testing Faktory connection pool...");
    let _test_conn = faktory_pool
        .get()
        .await
        .context("Failed to get test connection from pool")?;
    info!("Successfully connected to Faktory");

    // Create batch configuration and queue
    let batch_config = BatchConfig {
        max_batch_size,
        max_batch_delay_ms,
        auto_batch_enabled,
    };
    let batch_queue = Arc::new(Mutex::new(BatchQueue::new(batch_config.clone())));

    // Start background batch flusher
    let flusher_pool = faktory_pool.clone();
    let flusher_queue = batch_queue.clone();
    tokio::spawn(async move {
        batch_flusher(flusher_pool, flusher_queue, max_batch_delay_ms).await;
    });
    info!("Started batch flusher background task");

    // Create shared state
    let state = Arc::new(AppState {
        faktory_pool,
        batch_queue,
        batch_config,
    });

    // Build router
    let app = Router::new()
        .route("/health", axum::routing::get(health_handler))
        .route("/jobs/add", post(add_handler))
        .route("/jobs/subtract", post(subtract_handler))
        .route("/jobs/multiply", post(multiply_handler))
        .route("/jobs/divide", post(divide_handler))
        .route("/jobs/batch", post(batch_handler))
        .with_state(state);

    info!("Starting API service on {}", bind_addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
