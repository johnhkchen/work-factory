# Work Factory: Network Bottleneck Summary

## Quick Reference - 10x Performance Degradation Root Cause

### The Issue
Jobs process 10-100x slower on LAN/wireless (high-latency) networks compared to local deployment.

**Example**:
- Local (1ms RTT): 333 jobs/sec
- WAN (100ms RTT): 10 jobs/sec
- **Degradation**: 33x worse

### The Root Cause
**Each job enqueue = 1 network round-trip to Faktory**

```
Job 1 ──(100ms)──> Faktory ──(ACK)──> API returns
Job 2 ──(100ms)──> Faktory ──(ACK)──> API returns
Job 3 ──(100ms)──> Faktory ──(ACK)──> API returns
...

1000 jobs × 100ms = 100 SECONDS overhead
```

---

## Code Locations: Critical Network Communication

### 1. API Service Connection Pool
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
- **Lines**: 8-32 (FaktoryManager struct)
- **Line 77**: `Pool::builder(manager).max_size(50)`
- **Issue**: Hard-coded pool size, no timeout configuration

### 2. Job Enqueue - The Bottleneck
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
- **Lines**: 61-79 (enqueue_job function)
- **Key Problem**:
  ```rust
  let mut client = pool.get().await;  // Get connection from pool
  client.enqueue(job).await;           // BLOCKS for RTT seconds
  ```
- **Impact**: Full network RTT blocks the HTTP request

### 3. Handlers - Called Per Job
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
- **Lines**: 
  - 96-116 (add_handler)
  - 120-139 (subtract_handler) 
  - 143-166 (multiply_handler)
  - 170-193 (divide_handler)
- **Each handler**: Calls enqueue_job once
- **Result**: No batching support

### 4. Worker Configuration
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/worker-service/src/main.rs`
- **Lines**: 52-68 (Worker setup)
- **Issues**:
  - `.workers(50)` - hard-coded concurrency
  - No configurable poll interval
  - No timeout configuration
  - Unknown Faktory polling frequency

### 5. Serialization Format
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/crates/job-types/src/lib.rs`
- **Lines**: 19-53 (JobPayload enum)
- **Format**: JSON (text, no compression)
- **Size**: ~80 bytes per job after serialization

### 6. Nginx Proxy Configuration
- **File**: `/Users/johnchen/Documents/swe/repos/work-factory/nginx.conf`
- **Issues**:
  - Line 24: `keepalive 128;` - hard-coded pool
  - Lines 10-12: `tcp_nopush on` conflicts with latency optimization

---

## The 5 Critical Bottlenecks

### 🔴 CRITICAL: No Job Batching

**Impact**: Each job = 1 RTT to Faktory

```
Current: POST /jobs/add → Faktory → 100ms
Current: POST /jobs/add → Faktory → 100ms
Current: POST /jobs/add → Faktory → 100ms
...
1000 jobs = 100 seconds

Optimized: POST /jobs/batch (100 jobs) → Faktory → 100ms
Optimized: POST /jobs/batch (100 jobs) → Faktory → 100ms
...
1000 jobs = 10 seconds
```

**Fix Effort**: Medium
**Expected Improvement**: 10-100x

---

### 🟠 HIGH: Synchronous Enqueue Pattern

**Problem**: API handler blocked on Faktory response

```rust
client.enqueue(job).await;  // Blocks entire request
```

**Impact**: HTTP request latency = network latency + processing

**On 100ms RTT network**:
- One job enqueue = 100ms+ HTTP response time
- 50 concurrent requests = timeout or queue buildup

**Fix Effort**: Low
**Expected Improvement**: Better responsiveness, reduced timeouts

---

### 🟠 HIGH: No Configuration Options

**Missing Tunables**:
```
FAKTORY_CONNECTION_TIMEOUT    ❌ Missing
FAKTORY_READ_TIMEOUT          ❌ Missing
FAKTORY_WRITE_TIMEOUT         ❌ Missing
FAKTORY_MAX_CONNECTIONS       ❌ Hard-coded 50
WORKER_CONCURRENCY            ❌ Hard-coded 50
WORKER_POLL_INTERVAL          ❌ Library default (unknown)
TCP_KEEPALIVE_ENABLED         ❌ Missing
```

**Impact**: Can't tune for WAN/wireless conditions

**Fix Effort**: Very Low
**Expected Improvement**: Adaptability to different networks

---

### 🟡 MEDIUM: Unknown Worker Polling Frequency

**Problem**: Worker poll interval not visible/configurable
- Default likely 1-5 seconds
- Each poll = 1 RTT to Faktory
- Job waits 5+ seconds before network round-trip

**Example** (5 second default):
- Job enqueued at t=0
- Worker polls at t=5 (first time after job arrives)
- Network RTT = 100ms
- Job actually fetched at t=5.1 seconds
- Processing delay = 5 seconds (network effect)

**Fix Effort**: Low
**Expected Improvement**: Faster job pickup, especially important on slow networks

---

### 🟡 MEDIUM: No TCP Keepalive Configuration

**Problem**: Idle connections drop on firewalled networks

**Typical Scenario**:
- Worker on wireless network
- No traffic for 15 minutes
- Firewall drops connection
- Worker doesn't reconnect automatically
- Jobs queue up, workers appear dead

**Missing Configuration**:
```
TCP_KEEPALIVE_IDLE    ❌ Not configured
TCP_KEEPALIVE_INTERVAL❌ Not configured
TCP_KEEPALIVE_PROBES  ❌ Not configured
```

**Fix Effort**: Low
**Expected Improvement**: Robustness on unreliable networks

---

## Performance Degradation Calculation

### Variables

```
N = number of jobs = 1000
RTT = network round-trip time
  Local: 1ms
  LAN: 10ms
  WAN: 100ms
  Wireless: 50-200ms

P = processing time per job
  = ~1ms (JSON serialization, pool lookup, Faktory store)

T_total = (N × RTT) + (N × P) + overhead
```

### Local Network (1ms RTT)

```
T = (1000 × 0.001) + (1000 × 0.001) + 1
T = 1 + 1 + 1 = 3 seconds
Throughput = 1000 / 3 = 333 jobs/sec
```

### WAN Network (100ms RTT)

```
T = (1000 × 0.1) + (1000 × 0.001) + 1
T = 100 + 1 + 1 = 102 seconds
Throughput = 1000 / 102 = 9.8 jobs/sec

Degradation: 333 / 9.8 = 34x worse
```

### With Job Batching (100 jobs/batch, 100ms RTT)

```
Batches needed = 1000 / 100 = 10 batches
T = (10 × 0.1) + (1000 × 0.001) + 1
T = 1 + 1 + 1 = 3 seconds
Throughput = 1000 / 3 = 333 jobs/sec

Improvement: 333 / 9.8 = 34x (matches local performance)
```

---

## Network-Specific Impacts

### Wireless/LAN Issues

1. **Higher RTT variability** (10-100ms)
   - Makes average RTT unreliable
   - Timeouts more likely if set too aggressively

2. **Intermittent disconnections**
   - TCP keepalive becomes essential
   - Needs exponential backoff on reconnect

3. **Bandwidth constraints**
   - Compression matters more
   - Batch sizes should be larger

4. **Connection pooling overhead**
   - Each pool connection = 50-100ms setup time
   - Pool size should be smaller on slow networks

### Configuration Recommendations by Network Type

| Parameter | Local | LAN | WAN | Wireless |
|-----------|-------|-----|-----|----------|
| FAKTORY_MAX_CONNECTIONS | 50 | 30 | 15 | 10 |
| WORKER_CONCURRENCY | 50 | 30 | 15 | 10 |
| FAKTORY_CONNECTION_TIMEOUT | 5s | 10s | 15s | 20s |
| FAKTORY_READ_TIMEOUT | 10s | 30s | 60s | 90s |
| WORKER_POLL_INTERVAL | 100ms | 500ms | 1s | 2s |
| BATCH_SIZE (if implemented) | 100 | 50 | 20 | 10 |

---

## Implementation Roadmap

### Phase 1: Quick Fixes (Week 1)
- Add environment variables for configuration ✓ Easy, High Value
- Add connection timeouts ✓ Easy, High Value  
- Add TCP keepalive support ✓ Easy, Medium Value

**Expected Improvement**: Better stability, ability to tune for network

### Phase 2: Core Optimization (Week 2)
- Implement batch enqueue endpoint ✓ Medium, Critical Value

**Expected Improvement**: 10-100x throughput improvement on WAN

### Phase 3: Refinements (Week 3)
- Add compression support ✓ Medium, Medium Value
- Add metrics/observability ✓ Medium, High Value
- Improve worker polling ✓ Low, Medium Value

**Expected Improvement**: Visibility into bottlenecks, further tuning

### Phase 4: Advanced (Week 4+)
- Connection pooling optimization
- Request batching at Faktory client level
- Circuit breaker pattern
- Result callback/webhook support

---

## Testing the Fix

### Before Optimization Test

```bash
# Simulate 100ms WAN latency
sudo tc qdisc add dev eth0 root netem delay 100ms

# Benchmark
cd benchmark && cargo run --release --bin benchmark

# Expected on current code: ~10 jobs/sec for high-concurrency test
```

### After Optimization Test

```bash
# Same 100ms latency setup

# Use batch endpoint
curl -X POST http://localhost/jobs/batch \
  -H "Content-Type: application/json" \
  -d '{
    "jobs": [... 100 jobs ...]
  }' -w "@curl-format.txt" -o /dev/null

# Expected after batching: ~333 jobs/sec (matching local performance)
```

### Verification Script

```bash
#!/bin/bash
# test_network_optimization.sh

API_URL=${1:-http://localhost:3000}
JOBS=${2:-1000}
BATCH_SIZE=${3:-100}

echo "Testing $JOBS jobs in batches of $BATCH_SIZE"
echo "API: $API_URL"
echo ""

# Create batch request
BATCHES=$((JOBS / BATCH_SIZE))
for batch in $(seq 1 $BATCHES); do
  START=$(($batch * $BATCH_SIZE))
  echo "Batch $batch/$BATCHES (jobs $START-$(($START + $BATCH_SIZE)))"
  
  time curl -s -X POST "$API_URL/jobs/batch" \
    -H "Content-Type: application/json" \
    -d '{
      "jobs": ['$(
        for i in $(seq 0 $((BATCH_SIZE-1))); do
          J=$((START + i))
          echo '{"operation":"add","a":'$J',"b":'$J'}'
          [[ $i -lt $((BATCH_SIZE-1)) ]] && echo ','
        done
      )']}' > /dev/null
done

echo ""
echo "Total: $JOBS jobs in $BATCHES batches"
echo "Expected time: ~${BATCHES}00ms + processing"
```

---

## Files to Modify

### Must Modify
1. `/Users/johnchen/Documents/swe/repos/work-factory/crates/api-service/src/main.rs`
   - Add batch endpoint
   - Add config loading
   - Add timeouts

2. `/Users/johnchen/Documents/swe/repos/work-factory/crates/worker-service/src/main.rs`
   - Add config loading
   - Add keepalive configuration

3. `/Users/johnchen/Documents/swe/repos/work-factory/Cargo.toml`
   - Add config dependencies (if creating new config module)

### Should Modify
4. `/Users/johnchen/Documents/swe/repos/work-factory/docker-compose.yml`
   - Add environment variable examples
   - Show network-specific configurations

5. `/Users/johnchen/Documents/swe/repos/work-factory/README.md`
   - Document network optimization options
   - Add WAN deployment guidance

### Optional
6. `/Users/johnchen/Documents/swe/repos/work-factory/nginx.conf`
   - Tune for different network conditions

---

## Key Metrics to Monitor

After implementing fixes, monitor these metrics:

1. **Enqueue Latency** (per job)
   - Local: 1-5ms
   - LAN: 10-20ms
   - WAN: 50-150ms

2. **Throughput** (jobs/sec)
   - Local: 200-400 jobs/sec
   - LAN: 50-200 jobs/sec
   - WAN: 10-50 jobs/sec (after batching: 100-400)

3. **Pool Utilization**
   - Should not spike to max_connections frequently
   - Indicates backpressure

4. **Connection Success Rate**
   - Should remain >99% on stable networks
   - <95% may indicate timeout issues

5. **Average Job Processing Latency**
   - Should be constant regardless of network
   - If increases with network delay, batching not working

---

## Conclusion

The **10x performance degradation on WAN networks** is caused by:

1. **No job batching** (each job = 1 RTT) - 🔴 PRIMARY CAUSE
2. **Synchronous enqueue pattern** (blocks request)
3. **No configuration options** (can't tune)
4. **Unknown polling behavior** (job wait time)
5. **No keepalive configuration** (connection reliability)

**Quick Fix**: Implement batch enqueue endpoint → **10-100x improvement**

**Robust Fix**: Add configuration options + batching → **Adaptable to any network**

The system is well-architected for **local/LAN** but needs **network optimization** for **WAN/wireless** deployment.
