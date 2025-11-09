# Network Communication Analysis: Work Factory

## Executive Summary

This document analyzes the worker node communication architecture in the Work Factory distributed job queue system and identifies potential network-related bottlenecks that could cause 10x performance degradation on LAN/wireless connections compared to local deployments.

**Key Finding**: The system uses **Faktory** (a job queue broker) as the central communication hub. Workers and producers connect to Faktory via TCP, which is inherently inefficient for frequent network round-trips on high-latency connections.

---

## Architecture Overview

### System Components

```
┌─────────────────────────────────────────────────────────────┐
│ CENTRAL SERVER MACHINE                                      │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐                                           │
│  │  Faktory     │ (Job Queue Broker) - TCP Port 7419        │
│  │  (2 cores)   │                                           │
│  └──────┬───────┘                                           │
│         │ ├─ Stores all job state                           │
│         │ ├─ Processes acknowledgments                      │
│         │ └─ Manages job distribution                       │
│  ┌──────▼───────────────────────────────────────┐           │
│  │ ┌──────────────┐  ┌──────────────┐           │           │
│  │ │ API Service  │  │ Frontend UI  │ (Nginx)   │           │
│  │ │ (Enqueuer)   │  │ (Web UI)     │           │           │
│  │ └──────────────┘  └──────────────┘           │           │
│  └────────────────────────────────────────────┬─┘           │
│                                               │              │
└───────────────────────────────────────────────┼──────────────┘
                                                │ TCP Network
                                    ┌───────────┴────────────┐
                    ┌───────────────┴────────┐        ┌──────┴──────────┐
                    │                        │        │                 │
        ┌───────────▼──────────┐  ┌─────────▼──────┐ │  ┌────────────┬─▼────┐
        │ WORKER NODE 1        │  │ WORKER NODE 2  │ │  │ WORKER N   │      │
        │ (1 core, 30 jobs)    │  │ (1 core)       │ │  │ (1 core)   │ ...  │
        └───────────┬──────────┘  └─────────┬──────┘ │  └────────────┴──────┘
                    │                       │        │
         ┌──────────▼──────────┐  ┌────────▼───────┐
         │ Worker Service      │  │ Worker Service │
         │ - Fetches jobs      │  │ - Fetches jobs │
         │ - Processes locally │  │ - Processes    │
         │ - Reports results   │  │ - Reports      │
         └─────────────────────┘  └────────────────┘
```

### Communication Flow

1. **Job Enqueue** (API → Faktory):
   - API creates Job object
   - Serializes to JSON
   - Sends via TCP to Faktory port 7419
   - Awaits ACK before returning 202

2. **Job Distribution** (Faktory → Worker):
   - Worker maintains persistent TCP connection
   - Polls Faktory periodically with FETCH command
   - Faktory sends job payload (JSON)
   - Worker deserializes and executes locally

3. **Job Completion** (Worker → Faktory):
   - Worker sends ACK to Faktory
   - Faktory removes from queue
   - Optional: Result stored in Faktory

---

## Critical Network Communication Points

### 1. API Service - Connection Pool to Faktory
**File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
**Lines**: 8-32, 65-80

```rust
struct FaktoryManager {
    faktory_url: String,
}

// Pool configuration
let faktory_pool = Pool::builder(manager)
    .max_size(50)  // 50 concurrent connections to Faktory
    .build()
    .context("Failed to create Faktory connection pool")?;
```

**Network Implications**:
- Uses `deadpool` crate for connection pooling
- **Max 50 connections** to Faktory for API requests
- Each connection is a persistent TCP socket
- New connection overhead: ~10-100ms on WAN, ~1-5ms on LAN

**Bottleneck**: On high-latency networks, connection acquisition can block HTTP request handling.

### 2. Job Enqueue Operation
**File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
**Lines**: 61-79

```rust
async fn enqueue_job(pool: Pool<FaktoryManager>, payload: JobPayload) -> Result<String> {
    let job_type = payload.job_type();
    let args = payload.to_args()?;
    let job = Job::new(job_type, vec![args]);
    
    let mut client = pool
        .get()
        .await
        .context("Failed to get Faktory connection from pool")?;
    
    client.enqueue(job).await.context("Failed to enqueue job")?;
}
```

**Network Issues**:
- **Serialization**: JobPayload → JSON → serde_json::Value (lines 27-35 in job-types/lib.rs)
- **Round-trip latency**: Each job enqueue = 1 TCP round-trip to Faktory
- **No batching**: Individual jobs sent one-at-a-time
- **Blocking**: API handler waits for Faktory ACK before responding

**Impact**: 
- 1 job at 100ms RTT = 100ms API response time
- 10 jobs at 100ms RTT = 1 second cumulative

### 3. Worker Job Fetching
**File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/worker-service/src/main.rs`
**Lines**: 52-68

```rust
let mut worker = WorkerBuilder::default()
    .hostname("worker-service".to_string())
    .workers(50)  // 50 concurrent jobs per worker
    .register_fn("math_add", job_handler)
    .register_fn("math_subtract", job_handler)
    .register_fn("math_multiply", job_handler)
    .register_fn("math_divide", job_handler)
    .connect()
    .await?;

info!("Worker connected and ready to process jobs");
info!("Concurrency: 30 jobs per worker");  // Comment says 30 but code says 50!
```

**Network Issues**:
- **Faktory polling**: Worker maintains 1 persistent connection
- **Unknown polling frequency**: Faktory client library determines fetch interval (not configurable in code)
- **Worker concurrency**: 50 concurrent jobs means up to 50 parallel executions
- **No configuration** for connection timeouts, keepalive, or backoff strategies

### 4. Job Payload Serialization
**File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/job-types/src/lib.rs`
**Lines**: 19-53

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "args")]
pub enum JobPayload {
    Add(MathArgs),
    Subtract(MathArgs),
    Multiply(MathArgs),
    Divide(MathArgs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MathArgs {
    pub a: f64,
    pub b: f64,
    pub request_id: Option<String>,
}
```

**Network Impact**:
- Each job serialized to JSON (variable size: 40-200+ bytes)
- No compression configured
- Optional `request_id` adds 20-40 bytes per job when used
- String format is inefficient for numeric data over network

---

## Identified Network Bottlenecks

### 1. No Request Batching (CRITICAL)
**Severity**: 🔴 **CRITICAL** - This is THE primary bottleneck

**Problem**: Each job enqueue is an individual TCP request to Faktory
- 1 job = 1 RTT (round-trip time)
- 1000 jobs on 100ms latency = 100 seconds overhead just for network

**Current Code**:
```rust
// From api-service/src/main.rs lines 96-116
async fn add_handler(...) {
    let payload = JobPayload::Add(MathArgs { a, b, ... });
    match enqueue_job(state.faktory_pool.clone(), payload).await {
        Ok(job_id) => { /* single job enqueued */ }
    }
}
```

**Solution**: Batch multiple jobs per TCP request using Faktory's bulk enqueue capability

**Expected Impact**: 10-100x reduction in network RTTs on high-latency networks

---

### 2. No Connection Keepalive Configuration (HIGH)
**Severity**: 🟠 **HIGH**

**Problem**: TCP connections may timeout on slow/wireless networks

**Missing Configuration**:
- No TCP keepalive settings (TCP_KEEPALIVE)
- No application-level heartbeat/ping
- No socket timeout configuration
- No automatic reconnection on network interruption

**Impact**: Long-idle workers may drop connection after 15-30 minutes on firewall-heavy networks

---

### 3. Fixed Pool Size Without Tuning (HIGH)
**Severity**: 🟠 **HIGH**

**Problem**: Hard-coded pool sizes may not match network conditions
```rust
// api-service: max 50 connections
.max_size(50)

// worker-service: 50 concurrent jobs
.workers(50)
```

**Issue**: 
- No configuration for different network conditions
- README claims "30 concurrent jobs" but code has 50
- Nginx connection pool to API: 128 connections (hardcoded)
- No backpressure mechanism if Faktory slow to respond

---

### 4. Synchronous Enqueue Pattern (MEDIUM)
**Severity**: 🟡 **MEDIUM**

**Problem**: API blocks waiting for Faktory response
```rust
client.enqueue(job).await  // Blocks until Faktory ACKs
```

**Impact**: 
- HTTP request blocked for entire RTT
- Network delays directly impact HTTP response latency
- Can cause cascading timeouts if RTT > HTTP timeout

**Better approach**: Fire-and-forget or eventual consistency

---

### 5. No Compression (MEDIUM)
**Severity**: 🟡 **MEDIUM**

**Problem**: Job payloads sent uncompressed over network
- MathArgs (40 bytes) + metadata = ~80 bytes per job
- Faktory protocol overhead = ~20 bytes per job
- No gzip/compression configured anywhere

**Impact on 1M jobs**:
- Uncompressed: 80MB network traffic
- With gzip: ~8-16MB (80-90% reduction)
- On slow network: could save 5-30 seconds

---

### 6. Unknown Faktory Polling Behavior (MEDIUM)
**Severity**: 🟡 **MEDIUM**

**Problem**: Worker poll frequency not configurable
- Faktory library handles internally
- No visibility into heartbeat interval
- Likely 1-5 second default (unconfirmed)
- Could cause significant job processing delay on high-latency networks

**Impact**: 
- If poll interval = 5 seconds + 100ms RTT
- Job waits 5 seconds before network round-trip fetches it
- Actual latency = 5000ms + 100ms (not just 100ms)

---

### 7. Nginx Connection Pool Limitations (LOW-MEDIUM)
**Severity**: 🟡 **LOW-MEDIUM**

**File**: `/Users/johnchen/Documents/swe/repos/work-factory/nginx.conf`
**Lines**: 23-26

```nginx
upstream api {
    server api-service:3000;
    keepalive 128;  # Pool size
}
```

**Issues**:
- Hard-coded pool size (128)
- No tuning for network conditions
- `tcp_nodelay on` is good (low-latency TCP)
- `tcp_nopush on` conflicts with tcp_nodelay for high-throughput scenarios

---

### 8. No Response Result Storage (LOW)
**Severity**: 🟢 **LOW**

**Current**: Worker processes job locally, returns nothing to Faktory
- No result persistence
- No job completion feedback to API
- Results lost if not logged elsewhere

**Impact on Network**: Minimal directly, but affects use case flexibility

---

## Network Configuration Analysis

### Missing Configuration Options

The system lacks environment variables/config for:

```
FAKTORY_CONNECTION_TIMEOUT    # Missing - default unknown
FAKTORY_READ_TIMEOUT          # Missing - default unknown  
FAKTORY_WRITE_TIMEOUT         # Missing - default unknown
FAKTORY_KEEPALIVE_INTERVAL    # Missing - default unknown
FAKTORY_MAX_BATCH_SIZE        # Missing - no batching implemented
FAKTORY_MAX_CONNECTIONS       # Hard-coded to 50
FAKTORY_CONNECTION_POOL_SIZE  # Hard-coded to 50
WORKER_POLL_INTERVAL          # Missing - library default
WORKER_CONCURRENCY            # Hard-coded to 50
```

---

## Performance Impact Calculation: 10x Degradation Scenario

### Scenario: 1000 jobs enqueued over WAN (100ms RTT)

**Current Architecture (No Batching)**:
```
Time breakdown:
- 1000 sequential enqueues × 100ms RTT = 100,000 ms (100 seconds)
- API processing + serialization: ~1,000 ms (1ms per job)
- Faktory processing: ~1,000 ms
- Total: ~102 seconds

Throughput: 1000 jobs / 102 sec = 9.8 jobs/sec
```

**Local Network (1ms RTT)**:
```
- 1000 sequential enqueues × 1ms RTT = 1,000 ms
- API + serialization: ~1,000 ms
- Faktory: ~1,000 ms
- Total: ~3 seconds

Throughput: 1000 jobs / 3 sec = 333 jobs/sec

Degradation: 333 / 9.8 = 34x worse (not just 10x)
```

This matches the observed issue: **simple jobs see 10-100x slowdown on high-latency networks**.

---

## Detailed Code References

### Key Files and Line Numbers

| Component | File | Key Lines | Issue |
|-----------|------|-----------|-------|
| Connection Pool | `crates/api-service/src/main.rs` | 8-32, 65-80 | Hard-coded 50 connections, no timeout config |
| Job Enqueue | `crates/api-service/src/main.rs` | 61-79 | Synchronous, no batching |
| Job Serialization | `crates/job-types/src/lib.rs` | 19-53 | JSON format, no compression |
| Worker Config | `crates/worker-service/src/main.rs` | 52-68 | Hard-coded 50 workers, no polling config |
| Handlers | `crates/api-service/src/main.rs` | 96-116, 120-139, 143-166, 170-193 | Each calls enqueue_job individually |
| HTTP Client | `crates/frontend-service/src/main.rs` | 92-120 | No connection pooling, new client per request |
| Nginx Config | `nginx.conf` | 1-60 | Connection pool limits, buffer sizes |

---

## Configuration Recommendations

### Immediate Quick Wins

1. **Add Job Batching**
   - Group 10-100 jobs per TCP request
   - Expected: 10-100x reduction in network RTTs
   - Implementation effort: Medium

2. **Configure Connection Timeouts**
   - Add `FAKTORY_CONNECT_TIMEOUT` (default: 5s)
   - Add `FAKTORY_READ_TIMEOUT` (default: 30s)
   - Add `FAKTORY_WRITE_TIMEOUT` (default: 30s)
   - Implementation effort: Low

3. **Add TCP Keepalive**
   - Configure TCP keepalive on all sockets
   - Prevent timeout on idle connections
   - Implementation effort: Low

4. **Environment-based Pool Sizing**
   - Make `FAKTORY_MAX_CONNECTIONS` configurable
   - Default: 50 (good for 1-4 workers)
   - WAN: Maybe 10-20 (less concurrent, more efficient)
   - Implementation effort: Very Low

5. **Add Gzip Compression**
   - Compress job payloads in transit
   - Expected: 80-90% size reduction
   - Implementation effort: Medium

### Medium-term Improvements

6. **Implement Result Callback/Webhook**
   - Allow storing job results in Faktory
   - Enable job completion tracking
   - Implementation effort: Medium

7. **Add Metrics/Observability**
   - Track enqueue latency vs network latency
   - Monitor connection pool utilization
   - Implementation effort: Low-Medium

8. **Configurable Worker Poll Interval**
   - Expose Faktory client's poll frequency
   - Tune for different network conditions
   - Implementation effort: Low

9. **Circuit Breaker Pattern**
   - Fail fast if Faktory unreachable
   - Prevent cascading timeouts
   - Implementation effort: Medium

---

## Verification Tests

### Test 1: Measure Enqueue Latency by Distance

```bash
# Local (1ms RTT): Should be ~5-10ms per job
# LAN (10ms RTT): Should be ~15-20ms per job  
# WAN (100ms RTT): Should be ~110-120ms per job (THE BOTTLENECK)

curl -w "@curl-format.txt" -o /dev/null -s http://localhost/jobs/add -X POST
```

### Test 2: Monitor Connection Pool

```bash
# Check if pool is full (indicates backpressure)
netstat -an | grep ESTABLISHED | wc -l
```

### Test 3: Batch Enqueue Performance Comparison

Implement batch endpoint and compare:
- Current: 1000 jobs = 100-200 seconds (WAN)
- Batched (100/batch): 1000 jobs = 2-4 seconds (WAN)

---

## Summary Table: Network Bottlenecks

| Issue | Severity | RTT Impact | Recommended Fix | Effort |
|-------|----------|-----------|-----------------|--------|
| No job batching | 🔴 CRITICAL | **100-500ms per job** | Batch enqueue API | Medium |
| Sync enqueue | 🟠 HIGH | Full RTT blocking | Async/fire-forget | Low |
| No keepalive | 🟠 HIGH | Connection timeout | Config TCP options | Low |
| Fixed pool size | 🟠 HIGH | Backpressure/timeout | Make configurable | Low |
| Unknown poll freq | 🟡 MEDIUM | 5s+ job latency | Expose setting | Low |
| No compression | 🟡 MEDIUM | 20-30% bandwidth waste | Add gzip | Medium |
| Nginx pool limits | 🟡 MEDIUM | Connection starvation | Tune/configure | Low |
| No config options | 🟡 MEDIUM | Can't tune for network | Add env vars | Low |

---

## Conclusion

The **10x performance degradation on WAN/wireless networks** is primarily caused by:

1. **No job batching** - Each job = 1 network RTT (100ms RTT = 100ms per job)
2. **Synchronous enqueue** - API blocks waiting for Faktory response
3. **No tuning options** - Can't optimize for different network conditions
4. **Unknown polling** - Worker may wait seconds to fetch jobs

**Implementing batching alone would provide 10-100x improvement** on high-latency networks.

---

## Architecture Notes

- **Faktory Crate Version**: 0.13.1 (from Cargo.toml)
- **Connection Pool**: deadpool 0.12.1
- **HTTP Client**: reqwest 0.12.24 (in benchmarks, no pooling in main services)
- **API Framework**: Axum 0.8.6 (built on Tokio)

The system is well-architected for **local/LAN deployment** but needs optimization for **distributed/WAN scenarios**.
