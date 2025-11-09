# Work Factory: Code Reference - Network Communication Paths

This document provides quick reference to all network-related code with line numbers.

---

## API Service Network Communication

### File: `crates/api-service/src/main.rs`

#### Connection Pool Manager (Lines 8-32)
```rust
8:  struct FaktoryManager {
9:      faktory_url: String,
10: }
11:
12: impl Manager for FaktoryManager {
13:     type Type = Client;
14:     type Error = faktory::Error;
15:
16:     async fn create(&self) -> Result<Client, faktory::Error> {
17:         std::env::set_var("FAKTORY_URL", &self.faktory_url);
18:         Client::connect().await
19:     }
20:
21:     async fn recycle(
22:         &self,
23:         _conn: &mut Client,
24:         _metrics: &deadpool::managed::Metrics,
25:     ) -> RecycleResult<faktory::Error> {
26:         Ok(())
27:     }
28: }
```

**Issues**:
- No timeout configuration on connection creation (line 18)
- No keepalive configuration
- No connection validation in recycle (line 26)

---

#### Faktory Pool Creation (Lines 65-80)
```rust
65: // Create Faktory connection pool
66: let manager = FaktoryManager {
67:     faktory_url: faktory_url.clone(),
68: };
69: let faktory_pool = Pool::builder(manager)
70:     .max_size(50)  // 🔴 HARD-CODED: Not configurable
71:     .build()
72:     .context("Failed to create Faktory connection pool")?;
73:
74: info!("Created Faktory connection pool with max size 50");
```

**Issues**:
- Hard-coded `max_size(50)` - should be configurable
- No `timeout()` or `recycle_timeout()` configuration
- No connection validation
- Pool size inappropriate for WAN (should be 10-20 for high-latency)

---

#### Job Enqueue Function (Lines 61-79)
```rust
61: async fn enqueue_job(pool: Pool<FaktoryManager>, payload: JobPayload) -> Result<String> {
62:     // Create job
63:     let job_type = payload.job_type();
64:     let args = payload.to_args()?;
65:
66:     let job = Job::new(job_type, vec![args]);
67:     let job_id = job.id().to_string();
68:
69:     // Get a connection from the pool
70:     let mut client = pool
71:         .get()                           // 🔴 BLOCKS waiting for pool
72:         .await
73:         .context("Failed to get Faktory connection from pool")?;
74:
75:     // Push to Faktory
76:     client.enqueue(job).await           // 🔴 BLOCKS for full RTT
77:         .context("Failed to enqueue job")?;
78:
78:     info!("Enqueued job {} of type {}", job_id, job_type);
80:
81:     Ok(job_id)
82: }
```

**Issues**:
- Line 70-72: No timeout on pool.get() - can block indefinitely
- Line 76: No timeout on enqueue - can block indefinitely on slow network
- Synchronous (blocking) pattern blocks HTTP handler
- No batching support - each job = 1 network round-trip

---

#### Add Handler (Lines 96-116)
```rust
96: async fn add_handler(
97:     State(state): State<Arc<AppState>>,
98:     Json(req): Json<MathRequest>,
99: ) -> impl IntoResponse {
100:    let payload = JobPayload::Add(MathArgs {
101:        a: req.a,
102:        b: req.b,
103:        request_id: req.request_id,
104:    });
105:
106:    match enqueue_job(state.faktory_pool.clone(), payload).await {
107:        Ok(job_id) => {
108:            let response = JobResponse {
109:                job_id,
110:                message: format!("Job enqueued to add {} + {}", req.a, req.b),
111:            };
112:            (StatusCode::ACCEPTED, Json(response)).into_response()
113:        }
114:        Err(e) => {
115:            warn!("Failed to enqueue job: {:#}", e);
116:            let response = ErrorResponse {
117:                error: format!("Failed to enqueue job: {}", e),
118:            };
119:            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
120:        }
121:    }
122:}
```

**Issues**:
- Line 106: Calls enqueue_job for single job
- No batching
- HTTP response time = network RTT + processing

**Same pattern for**:
- Lines 120-139: subtract_handler
- Lines 143-166: multiply_handler
- Lines 170-193: divide_handler

---

#### Configuration Loading (Lines 43-50)
```rust
43: let faktory_url =
44:     std::env::var("FAKTORY_URL").unwrap_or_else(|_| "tcp://localhost:7419".to_string());
45: let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
46:
47: info!("Faktory URL: {}", faktory_url);
48: info!("Binding to: {}", bind_addr);
```

**Issues**:
- Only FAKTORY_URL and BIND_ADDR configurable
- Missing: timeouts, pool size, batch size, compression
- No validation of URL format

---

## Worker Service Network Communication

### File: `crates/worker-service/src/main.rs`

#### Worker Configuration (Lines 52-68)
```rust
52: let mut worker = WorkerBuilder::default()
53:     .hostname("worker-service".to_string())
54:     .workers(50)  // 🔴 HARD-CODED: Not configurable
55:     .register_fn("math_add", job_handler)
56:     .register_fn("math_subtract", job_handler)
57:     .register_fn("math_multiply", job_handler)
58:     .register_fn("math_divide", job_handler)
59:     .connect()    // 🔴 No timeout configuration
60:     .await?;
61:
62: info!("Worker connected and ready to process jobs");
63: info!("Concurrency: 30 jobs per worker");  // 🔴 WRONG: says 30 but code uses 50
```

**Issues**:
- Line 54: Hard-coded `.workers(50)` - should be configurable
- Line 59: `.connect()` has no timeout (could hang forever)
- Line 63: Documentation says 30 but code says 50 (mismatch)
- No configuration for poll interval
- No configuration for heartbeat/keepalive

---

#### Job Handler (Lines 22-54)
```rust
22: async fn job_handler(job: Job) -> Result<()> {
23:    let job_type = job.kind();
24:
25:    // Get the first argument (our job payload)
26:    let args_value = job
27:        .args()
28:        .get(0)
29:        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Job missing arguments"))?
30:        .clone();
31:
32:    // Parse into our typed JobPayload
33:    let payload = JobPayload::from_job_type(job_type, args_value).map_err(|e| {
34:        io::Error::new(
35:            io::ErrorKind::InvalidInput,
36:            format!("Failed to parse job payload: {}", e),
37:        )
38:    })?;
39:
40:    // Dispatch to the appropriate handler
41:    let result = match payload {
42:        JobPayload::Add(args) => handle_add(args),
43:        JobPayload::Subtract(args) => handle_subtract(args),
43:        JobPayload::Multiply(args) => handle_multiply(args),
45:        JobPayload::Divide(args) => handle_divide(args),
46:    };
47:
48:    match result {
49:        Ok(_value) => Ok(()),
50:        Err(e) => {
51:            error!("Job failed: {:#}", e);
52:            Err(e)
53:        }
54:    }
55:}
```

**No network issues here** - job processing is local

---

## Job Types Serialization

### File: `crates/job-types/src/lib.rs`

#### JobPayload Definition (Lines 8-18)
```rust
8:  #[derive(Debug, Clone, Serialize, Deserialize)]
9:  #[serde(tag = "type", content = "args")]
10: pub enum JobPayload {
11:     /// Add two numbers together
12:     Add(MathArgs),
13:     /// Subtract two numbers
14:     Subtract(MathArgs),
15:     /// Multiply two numbers
16:     Multiply(MathArgs),
17:     /// Divide two numbers
18:     Divide(MathArgs),
19: }
```

**Issues**:
- Uses JSON serialization (text format, not binary)
- No compression support
- Enum with tagged representation adds overhead

---

#### MathArgs Definition (Lines 58-65)
```rust
58: #[derive(Debug, Clone, Serialize, Deserialize)]
59: pub struct MathArgs {
60:     pub a: f64,
61:     pub b: f64,
62:     /// Optional identifier for tracking the operation
63:     pub request_id: Option<String>,
64: }
```

**Issues**:
- f64 serialized as JSON text (12-17 bytes per number)
- Optional String can add 20-100 bytes
- Total: 40-200 bytes per job (could be 16 bytes in binary)

---

#### Job Type Conversion (Lines 21-33)
```rust
21: pub fn job_type(&self) -> &'static str {
22:     match self {
23:         JobPayload::Add(_) => "math_add",
24:         JobPayload::Subtract(_) => "math_subtract",
25:         JobPayload::Multiply(_) => "math_multiply",
26:         JobPayload::Divide(_) => "math_divide",
27:     }
28: }
29:
29: pub fn to_args(&self) -> Result<serde_json::Value> {
30:     let args = match self {
31:         JobPayload::Add(args) => serde_json::to_value(args)?,
32:         JobPayload::Subtract(args) => serde_json::to_value(args)?,
33:         JobPayload::Multiply(args) => serde_json::to_value(args)?,
34:         JobPayload::Divide(args) => serde_json::to_value(args)?,
35:     };
36:     Ok(args)
37: }
```

**Issues**:
- Line 31-34: JSON serialization happens on every enqueue
- No caching of serialized form
- No compression support

---

## Nginx Configuration

### File: `nginx.conf`

#### Connection Pool Configuration (Lines 23-26)
```nginx
23: upstream api {
24:     server api-service:3000;
25:     keepalive 128;  # 🟡 HARD-CODED: Not tuned for WAN
26: }
```

**Issues**:
- Hard-coded pool size (128)
- No configuration for timeout
- Should be smaller for high-latency networks

---

#### HTTP Settings (Lines 8-15)
```nginx
8:  # High performance settings
9:  sendfile on;
10: tcp_nopush on;      # 🟡 Can increase latency
11: tcp_nodelay on;     # ✓ Good for low-latency
12: keepalive_timeout 65;
13: keepalive_requests 1000;
14:
15: # Large buffers for high throughput
16: client_body_buffer_size 128k;
```

**Issues**:
- Line 10: `tcp_nopush` conflicts with `tcp_nodelay` on latency-sensitive workloads
- Lines 16-19: Buffers optimized for local network, not WAN

---

#### Proxy Settings (Lines 41-48)
```nginx
41: location /jobs/ {
42:     proxy_pass http://api;
43:     proxy_http_version 1.1;
44:     proxy_set_header Connection "";
45:     proxy_set_header Host $host;
46:
47:     # No buffering for streaming responses
48:     proxy_buffering off;
49: }
```

**Good**: 
- Line 43: Uses HTTP/1.1 (connection reuse)
- Line 44: Connection header cleared (persistent connections)
- Line 48: Buffering off (low latency)

---

## Docker Compose Configuration

### File: `docker-compose.yml`

#### Worker Service (Lines 22-35)
```yaml
22: worker-service:
23:   build:
24:     context: .
25:     dockerfile: Dockerfile.worker
26:   environment:
27:     - FAKTORY_URL=tcp://faktory:7419
28:     - RUST_LOG=warn
29:   depends_on:
30:     faktory:
31:       condition: service_healthy
32:   stop_grace_period: 35s
33:   deploy:
34:     resources:
35:       limits:
36:         cpus: "1.0"
```

**Issues**:
- Line 27: No environment variables for timeouts, concurrency, poll interval
- Line 28: RUST_LOG=warn - no visibility into network issues

---

### File: `docker-compose.worker.yml`

#### Remote Worker Configuration (Lines 4-16)
```yaml
4: services:
5:   worker-service:
6:     build:
7:       context: .
8:       dockerfile: Dockerfile.worker
9:     environment:
10:      - FAKTORY_URL=tcp://${FAKTORY_SERVER_IP:-localhost}:7419
11:      - RUST_LOG=warn
12:    stop_grace_period: 35s
13:    deploy:
14:      resources:
15:        limits:
16:          cpus: "1.0"
```

**Issues**:
- Line 10: Hard-coded TCP port 7419 (can't change)
- Line 11: No network-specific environment variables
- No DNS configuration
- No network policy configuration

---

## Dependencies - Cargo.toml

### File: `crates/api-service/Cargo.toml`

```toml
11: # Faktory client
12: faktory = "0.13.1"
13:
14: # Connection pooling
15: deadpool = "0.12.1"
```

**Analysis**:
- faktory 0.13.1: No timeout configuration exposed
- deadpool 0.12.1: Supports timeout() and recycle_timeout()
- Missing: socket2 (for TCP keepalive), flate2 (for compression)

---

### File: `crates/worker-service/Cargo.toml`

```toml
10: # Faktory worker
11: faktory = "0.13.1"
```

**Analysis**:
- Same faktory version, no timeout config available
- No dependencies for keepalive or compression

---

## Summary: Files Needing Changes

| File | Lines | Issue | Fix Effort |
|------|-------|-------|-----------|
| api-service/src/main.rs | 70 | Hard-coded pool size | Low |
| api-service/src/main.rs | 76 | No timeout on enqueue | Low |
| api-service/src/main.rs | 96-193 | No batch endpoint | Medium |
| api-service/src/main.rs | 43-50 | Missing config options | Very Low |
| worker-service/src/main.rs | 54 | Hard-coded workers | Low |
| worker-service/src/main.rs | 59 | No connection timeout | Low |
| job-types/src/lib.rs | 58-64 | JSON serialization | Medium |
| nginx.conf | 25 | Hard-coded pool | Low |
| nginx.conf | 10-11 | tcp settings conflict | Low |
| docker-compose.yml | 26-28 | No env vars | Very Low |
| docker-compose.worker.yml | 9-11 | No env vars | Very Low |

---

## Quick Navigation

### Find Connection Pool Configuration
Search for: `.max_size(50)` in `api-service/src/main.rs` line 70

### Find Enqueue Operation
Search for: `fn enqueue_job` in `api-service/src/main.rs` line 61

### Find Handler Functions  
Search for: `fn add_handler` in `api-service/src/main.rs` line 96

### Find Worker Configuration
Search for: `.workers(50)` in `worker-service/src/main.rs` line 54

### Find Serialization
Search for: `pub fn to_args` in `job-types/src/lib.rs` line 29

### Find Nginx Proxy
Search for: `upstream api` in `nginx.conf` line 23

---

## Network Communication Timeline

For a single job enqueue on 100ms RTT network:

```
t=0ms     POST /jobs/add received by API
t=1ms     JSON deserialized
t=2ms     JobPayload created
t=3ms     Acquire connection from pool (may wait)
t=5ms     JSON serialized to send to Faktory
t=5ms     TCP SYN to Faktory (network)
t=55ms    TCP SYN-ACK received
t=55ms    Job data sent
t=105ms   Faktory ACK received  🔴 100ms network latency
t=106ms   Pool connection released
t=107ms   HTTP 202 response sent
```

**Total HTTP latency: 107ms** (mostly network RTT)

With batching:
```
t=0ms     POST /jobs/batch with 100 jobs
t=2ms     Parse 100 job definitions
t=5ms     Serialize 100 jobs to JSON
t=5ms     One TCP connection reused
t=105ms   All 100 jobs enqueued
t=107ms   HTTP 202 response sent
```

**Total HTTP latency: 107ms for 100 jobs** (vs 100 × 107ms = 10,700ms individually)

**Improvement: 100x for same response time**
