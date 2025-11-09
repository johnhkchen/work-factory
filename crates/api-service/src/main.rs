use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use deadpool::managed::{Manager, Pool, RecycleResult};
use faktory::{Client, Job};
use job_types::{JobPayload, MathArgs};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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

/// Shared application state
#[derive(Clone)]
struct AppState {
    faktory_pool: Pool<FaktoryManager>,
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

    match enqueue_job(state.faktory_pool.clone(), payload).await {
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

    match enqueue_job(state.faktory_pool.clone(), payload).await {
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

    match enqueue_job(state.faktory_pool.clone(), payload).await {
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

    match enqueue_job(state.faktory_pool.clone(), payload).await {
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

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "api-service"
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Configuration
    let faktory_url =
        std::env::var("FAKTORY_URL").unwrap_or_else(|_| "tcp://localhost:7419".to_string());
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    info!("Faktory URL: {}", faktory_url);
    info!("Binding to: {}", bind_addr);

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

    // Create shared state
    let state = Arc::new(AppState { faktory_pool });

    // Build router
    let app = Router::new()
        .route("/health", axum::routing::get(health_handler))
        .route("/jobs/add", post(add_handler))
        .route("/jobs/subtract", post(subtract_handler))
        .route("/jobs/multiply", post(multiply_handler))
        .route("/jobs/divide", post(divide_handler))
        .with_state(state);

    info!("Starting API service on {}", bind_addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
