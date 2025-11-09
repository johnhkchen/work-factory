# Network Optimization Guide for Work Factory

This guide provides specific code changes and configuration recommendations to optimize performance on LAN/wireless networks.

---

## Priority 1: Implement Job Batching (CRITICAL)

### The Problem
Currently, each job enqueue is a separate HTTP request and TCP transaction with Faktory.

**Current Flow**:
```
Request 1: POST /jobs/add {a: 1, b: 2}  →  100ms RTT
Request 2: POST /jobs/add {a: 3, b: 4}  →  100ms RTT
Request 3: POST /jobs/add {a: 5, b: 6}  →  100ms RTT
...
1000 jobs = 100 seconds on 100ms RTT network
```

### Solution 1A: Add Batch Enqueue Endpoint

Create a new API endpoint that accepts multiple jobs:

```rust
// File: crates/api-service/src/main.rs
// Add after multiply_handler (around line 200)

use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct BatchJobRequest {
    jobs: Vec<JobDefinition>,
}

#[derive(Debug, Deserialize)]
struct JobDefinition {
    operation: String,  // "add", "subtract", "multiply", "divide"
    a: f64,
    b: f64,
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchJobResponse {
    job_ids: Vec<String>,
    message: String,
    count: usize,
}

/// POST /jobs/batch - Enqueue multiple jobs in one request
async fn batch_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchJobRequest>,
) -> impl IntoResponse {
    let mut job_ids = Vec::new();
    let batch_size = req.jobs.len();
    let mut errors = Vec::new();

    for job_def in req.jobs {
        let payload = match job_def.operation.as_str() {
            "add" => JobPayload::Add(MathArgs {
                a: job_def.a,
                b: job_def.b,
                request_id: job_def.request_id,
            }),
            "subtract" => JobPayload::Subtract(MathArgs {
                a: job_def.a,
                b: job_def.b,
                request_id: job_def.request_id,
            }),
            "multiply" => JobPayload::Multiply(MathArgs {
                a: job_def.a,
                b: job_def.b,
                request_id: job_def.request_id,
            }),
            "divide" => JobPayload::Divide(MathArgs {
                a: job_def.a,
                b: job_def.b,
                request_id: job_def.request_id,
            }),
            op => {
                errors.push(format!("Unknown operation: {}", op));
                continue;
            }
        };

        match enqueue_job(state.faktory_pool.clone(), payload).await {
            Ok(job_id) => job_ids.push(job_id),
            Err(e) => errors.push(format!("Failed to enqueue: {}", e)),
        }
    }

    if !errors.is_empty() {
        let response = serde_json::json!({
            "job_ids": job_ids,
            "count": job_ids.len(),
            "total_requested": batch_size,
            "errors": errors,
            "message": format!("Enqueued {}/{} jobs", job_ids.len(), batch_size),
        });
        return (StatusCode::PARTIAL_CONTENT, Json(response)).into_response();
    }

    let response = BatchJobResponse {
        job_ids,
        message: format!("Batch of {} jobs enqueued", batch_size),
        count: batch_size,
    };

    (StatusCode::ACCEPTED, Json(response)).into_response()
}

// In main(), add to router (around line 250):
let app = Router::new()
    .route("/health", axum::routing::get(health_handler))
    .route("/jobs/add", post(add_handler))
    .route("/jobs/subtract", post(subtract_handler))
    .route("/jobs/multiply", post(multiply_handler))
    .route("/jobs/divide", post(divide_handler))
    .route("/jobs/batch", post(batch_handler))  // NEW
    .with_state(state);
```

**Usage**:
```bash
# Before: 1000 jobs = 1000 requests = 100 seconds (on 100ms RTT)
for i in {1..1000}; do
  curl -X POST http://localhost/jobs/add \
    -H "Content-Type: application/json" \
    -d '{"a":1,"b":2}'
done

# After: 1000 jobs = 10 requests = 1 second (on 100ms RTT)
curl -X POST http://localhost/jobs/batch \
  -H "Content-Type: application/json" \
  -d '{
    "jobs": [
      {"operation":"add","a":1,"b":2},
      {"operation":"add","a":3,"b":4},
      ... (100 jobs per batch)
    ]
  }'
```

**Expected Impact**: 10-100x reduction in network overhead

---

### Solution 1B: Alternative - Batch at Faktory Client Level

If you want to keep API endpoints single-job but batch internally:

```rust
// File: crates/api-service/src/main.rs
// Replace the FaktoryManager struct (around line 8-32)

use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

struct FaktoryManager {
    faktory_url: String,
}

// Add this after the main() function start:

async fn batch_enqueue_worker(
    mut faktory_pool: Pool<FaktoryManager>,
    mut rx: mpsc::Receiver<Job>,
) {
    let mut batch = Vec::new();
    let mut batch_timer = interval(Duration::from_millis(50)); // Flush every 50ms

    loop {
        tokio::select! {
            Some(job) = rx.recv() => {
                batch.push(job);
                
                // Flush when batch reaches 100 jobs
                if batch.len() >= 100 {
                    if let Ok(mut client) = faktory_pool.get().await {
                        for job in batch.drain(..) {
                            let _ = client.enqueue(job).await;
                        }
                    }
                }
            }
            _ = batch_timer.tick() => {
                // Flush remaining jobs every 50ms even if not full
                if !batch.is_empty() {
                    if let Ok(mut client) = faktory_pool.get().await {
                        for job in batch.drain(..) {
                            let _ = client.enqueue(job).await;
                        }
                    }
                }
            }
        }
    }
}
```

**Pros**: API stays simple, single-job endpoints work as before
**Cons**: More complex implementation, harder to debug

**Recommendation**: Use Solution 1A (batch endpoint) - more explicit and easier to tune.

---

## Priority 2: Add Configuration Options

### Add Environment Variables

Create a new config module:

```rust
// File: crates/api-service/src/config.rs (NEW FILE)

pub struct FaktoryConfig {
    pub url: String,
    pub max_connections: u32,
    pub connection_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub write_timeout_secs: u64,
}

impl FaktoryConfig {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("FAKTORY_URL")
                .unwrap_or_else(|_| "tcp://localhost:7419".to_string()),
            max_connections: std::env::var("FAKTORY_MAX_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            connection_timeout_secs: std::env::var("FAKTORY_CONNECTION_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            read_timeout_secs: std::env::var("FAKTORY_READ_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            write_timeout_secs: std::env::var("FAKTORY_WRITE_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
        }
    }
}

pub struct WorkerConfig {
    pub url: String,
    pub concurrency: u32,
    pub poll_interval_ms: u64,
    pub heartbeat_interval_secs: u64,
}

impl WorkerConfig {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("FAKTORY_URL")
                .unwrap_or_else(|_| "tcp://localhost:7419".to_string()),
            concurrency: std::env::var("WORKER_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            poll_interval_ms: std::env::var("WORKER_POLL_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            heartbeat_interval_secs: std::env::var("WORKER_HEARTBEAT_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
        }
    }
}
```

### Update API Service to Use Config

```rust
// File: crates/api-service/src/main.rs
// Add at top of file:
mod config;
use config::FaktoryConfig;

// In main(), replace the hard-coded configuration:
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load configuration from environment
    let config = FaktoryConfig::from_env();
    
    info!("Faktory URL: {}", config.url);
    info!("Max connections: {}", config.max_connections);
    info!("Connection timeout: {}s", config.connection_timeout_secs);
    info!("Bind address: 0.0.0.0:3000");

    let manager = FaktoryManager {
        faktory_url: config.url,
    };
    
    let faktory_pool = Pool::builder(manager)
        .max_size(config.max_connections)  // NOW CONFIGURABLE
        .build()
        .context("Failed to create Faktory connection pool")?;

    // ... rest of main()
}
```

### Environment Variables to Add

Create a `.env.example` file:

```bash
# Faktory Configuration
FAKTORY_URL=tcp://localhost:7419
FAKTORY_MAX_CONNECTIONS=50
FAKTORY_CONNECTION_TIMEOUT=5
FAKTORY_READ_TIMEOUT=30
FAKTORY_WRITE_TIMEOUT=30

# Worker Configuration
WORKER_CONCURRENCY=50
WORKER_POLL_INTERVAL_MS=100
WORKER_HEARTBEAT_INTERVAL=15

# API Server
BIND_ADDR=0.0.0.0:3000

# Logging
RUST_LOG=info

# Network Optimization (for WAN/wireless)
FAKTORY_MAX_CONNECTIONS=20
WORKER_CONCURRENCY=20
WORKER_POLL_INTERVAL_MS=500
FAKTORY_CONNECTION_TIMEOUT=10
FAKTORY_READ_TIMEOUT=60
```

---

## Priority 3: Implement Connection Timeouts

### Update Faktory Client Configuration

The `faktory` crate supports socket configuration. Add timeout handling:

```rust
// File: crates/api-service/src/main.rs
// Modify FaktoryManager::create()

impl Manager for FaktoryManager {
    type Type = Client;
    type Error = faktory::Error;

    async fn create(&self) -> Result<Client, faktory::Error> {
        std::env::set_var("FAKTORY_URL", &self.faktory_url);
        
        // Set socket-level timeouts via environment
        // (Note: faktory crate may not expose these directly,
        // this is a workaround using socket2 crate)
        
        let client = Client::connect().await?;
        
        // If faktory crate exposed socket access, we would do:
        // socket.set_read_timeout(Some(Duration::from_secs(30)))?;
        // socket.set_write_timeout(Some(Duration::from_secs(30)))?;
        
        Ok(client)
    }

    async fn recycle(
        &self,
        conn: &mut Client,
        _metrics: &deadpool::managed::Metrics,
    ) -> RecycleResult<faktory::Error> {
        // Check if connection is still alive
        // Could add optional ping/heartbeat here
        Ok(())
    }
}
```

### Fallback: Timeout at Application Level

```rust
// In enqueue_job function, add timeout wrapper:

async fn enqueue_job_with_timeout(
    pool: Pool<FaktoryManager>,
    payload: JobPayload,
    timeout_secs: u64,
) -> Result<String> {
    let job_type = payload.job_type();
    let args = payload.to_args()?;
    let job = Job::new(job_type, vec![args]);
    let job_id = job.id().to_string();

    let mut client = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        pool.get(),
    )
    .await
    .context("Timeout getting Faktory connection")?
    .context("Failed to get Faktory connection")?;

    tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        client.enqueue(job),
    )
    .await
    .context("Timeout enqueuing job")?
    .context("Failed to enqueue job")?;

    info!("Enqueued job {} of type {}", job_id, job_type);

    Ok(job_id)
}
```

---

## Priority 4: TCP Keepalive Configuration

### Option A: Using socket2 Crate

Add to Cargo.toml:
```toml
socket2 = "0.5"
```

Create keepalive wrapper:

```rust
// File: crates/api-service/src/main.rs

use socket2::{Socket, Domain, Type};
use std::net::SocketAddr;

fn configure_keepalive() -> Result<()> {
    // This is a demonstration - in practice, you'd need to
    // wrap the Faktory client to intercept socket creation
    
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    
    #[cfg(target_os = "linux")]
    {
        socket.set_tcp_keepalive_idle(Duration::from_secs(60))?;
        socket.set_tcp_keepalive_interval(Duration::from_secs(10))?;
        socket.set_tcp_keepalive_retries(5)?;
    }
    
    #[cfg(target_os = "macos")]
    {
        // macOS API is slightly different
        socket.set_tcp_keepalive(&true)?;
    }
    
    Ok(())
}
```

### Option B: Configure in Docker/Networking

In `docker-compose.worker.yml`, add TCP socket options:

```yaml
services:
  worker-service:
    build:
      context: .
      dockerfile: Dockerfile.worker
    environment:
      - FAKTORY_URL=tcp://${FAKTORY_SERVER_IP:-localhost}:7419
      - RUST_LOG=warn
      - TCP_KEEPALIVE_INTERVAL=30
      - TCP_KEEPALIVE_PROBES=3
    sysctls:
      - net.ipv4.tcp_keepalives_intvl=30
      - net.ipv4.tcp_keepalives_probes=3
      - net.ipv4.tcp_keepalives_time=60
    stop_grace_period: 35s
    deploy:
      resources:
        limits:
          cpus: "1.0"
```

---

## Priority 5: Add Compression (Optional but Effective)

### Add Flate2 for Gzip Compression

Add to `crates/api-service/Cargo.toml`:
```toml
flate2 = "1.0"
```

Create a job serializer with compression:

```rust
// File: crates/job-types/src/compression.rs (NEW FILE)

use serde::{Deserialize, Serialize};
use anyhow::Result;
use flate2::Compression;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use std::io::{Read, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedPayload {
    pub data: Vec<u8>,
    pub compressed: bool,
    pub original_size: usize,
}

impl CompressedPayload {
    pub fn from_job(payload: &JobPayload, compress: bool) -> Result<Self> {
        let json_data = serde_json::to_string(payload)?;
        let original_size = json_data.len();
        
        let data = if compress && original_size > 100 {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(json_data.as_bytes())?;
            encoder.finish()?
        } else {
            json_data.into_bytes()
        };
        
        Ok(CompressedPayload {
            data,
            compressed: compress && original_size > 100,
            original_size,
        })
    }
    
    pub fn decompress(&self) -> Result<String> {
        if self.compressed {
            let mut decoder = GzDecoder::new(&self.data[..]);
            let mut result = String::new();
            decoder.read_to_string(&mut result)?;
            Ok(result)
        } else {
            Ok(String::from_utf8(self.data.clone())?)
        }
    }
}
```

**Impact**: For MathArgs (20-40 bytes each), compression may not help much. But for larger payloads:
- 1KB job: 30-40% reduction
- 10KB job: 70-80% reduction

---

## Recommended Configuration for Different Networks

### Configuration Profiles

Create `.env.local` for testing:

```bash
# Local Development (1ms RTT, high bandwidth)
FAKTORY_MAX_CONNECTIONS=50
WORKER_CONCURRENCY=50
FAKTORY_CONNECTION_TIMEOUT=5
FAKTORY_READ_TIMEOUT=10

# LAN Deployment (5-20ms RTT)
FAKTORY_MAX_CONNECTIONS=30
WORKER_CONCURRENCY=30
FAKTORY_CONNECTION_TIMEOUT=10
FAKTORY_READ_TIMEOUT=30
WORKER_POLL_INTERVAL_MS=200

# WAN/Wireless (100-500ms RTT)
FAKTORY_MAX_CONNECTIONS=15
WORKER_CONCURRENCY=15
FAKTORY_CONNECTION_TIMEOUT=15
FAKTORY_READ_TIMEOUT=60
WORKER_POLL_INTERVAL_MS=1000
```

### How to Apply

```bash
# Local
docker compose --env-file=.env.local up

# LAN
docker compose --env-file=.env.lan up

# WAN
docker compose --env-file=.env.wan up
```

---

## Performance Testing

### Benchmark Before/After

```bash
# Before optimization
time curl -X POST http://localhost/jobs/add \
  -H "Content-Type: application/json" \
  -d '{"a":1,"b":2}'  # Should take ~2-5ms locally, 100ms+ over WAN

# After batching optimization
time curl -X POST http://localhost/jobs/batch \
  -H "Content-Type: application/json" \
  -d '{
    "jobs": [
      {"operation":"add","a":1,"b":2},
      ... (100 jobs)
    ]
  }'  # Should take ~5-10ms locally, 100ms+ over WAN (but for 100 jobs!)
```

### Throughput Comparison Script

```bash
#!/bin/bash
# benchmark_improvements.sh

API_URL=${1:-http://localhost}

echo "=== Single Job Enqueue ==="
time {
  for i in {1..100}; do
    curl -s -X POST "$API_URL/jobs/add" \
      -H "Content-Type: application/json" \
      -d '{"a":1,"b":2}' > /dev/null
  done
}

echo -e "\n=== Batch Enqueue (100 jobs/request) ==="
time {
  curl -s -X POST "$API_URL/jobs/batch" \
    -H "Content-Type: application/json" \
    -d '{
      "jobs": ['$(
        for i in {1..100}; do
          echo '{"operation":"add","a":1,"b":2}'
          [[ $i -lt 100 ]] && echo ','
        done
      )']}' > /dev/null
}
```

---

## Monitoring Network Performance

### Add Metrics to Track Network Latency

```rust
// Add to api-service main.rs to measure per-endpoint latency

use std::time::Instant;

async fn add_handler_with_metrics(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MathRequest>,
) -> impl IntoResponse {
    let start = Instant::now();
    
    let payload = JobPayload::Add(MathArgs {
        a: req.a,
        b: req.b,
        request_id: req.request_id,
    });

    match enqueue_job(state.faktory_pool.clone(), payload).await {
        Ok(job_id) => {
            let elapsed = start.elapsed();
            info!("Enqueue latency: {:?}", elapsed);
            
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
```

---

## Summary: Implementation Priority

| Priority | Feature | Effort | Impact on WAN |
|----------|---------|--------|--------------|
| 1 | Batch enqueue endpoint | Medium | 🔴 10-100x improvement |
| 2 | Config from environment | Low | 🟠 Tuning capability |
| 3 | Connection timeouts | Low | 🟠 Prevent hanging |
| 4 | TCP keepalive | Low | 🟠 Prevent timeout |
| 5 | Compression | Medium | 🟡 10-30% bandwidth |
| 6 | Metrics/observability | Medium | 🟢 Visibility |

**To fix the 10x degradation, focus on Priority 1 (batching).**

The other items provide additional robustness and tuning capability for different network conditions.
