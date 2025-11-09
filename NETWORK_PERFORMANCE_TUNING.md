# Network Performance Tuning Guide

## Understanding the Bottleneck

When you saw **2k jobs/sec on LAN** despite batching being enabled, the bottleneck wasn't the batching system—it was the **worker fetch pattern**.

### The Problem

**Job Flow:**
```
Client → API → Faktory (✅ BATCHED)
Faktory → Worker (❌ NOT BATCHED - this was the bottleneck!)
```

Workers fetch jobs from Faktory **one at a time** over the network:
1. Worker finishes job
2. Worker sends FETCH request to Faktory (network round-trip)
3. Faktory responds with 1 job
4. Worker processes job
5. Repeat

**With 50ms network latency:**
- Each fetch cycle = 50ms (network) + processing time
- Even if processing is instant, you're limited to ~20 fetches/sec per worker slot
- With 50 concurrent workers: 50 × 20 = **1,000 jobs/sec theoretical max**
- Your 2k/sec was actually pretty good!

### The Solution: High Concurrency

Increase worker concurrency dramatically to **hide network latency**:

```
With 50 workers:
- 1 worker waiting on network (50ms)
- 49 workers processing jobs
= Bottlenecked by network

With 500 workers:
- 10 workers waiting on network (50ms each)
- 490 workers processing jobs
= Much higher throughput!
```

## Configuration

### WORKER_CONCURRENCY Environment Variable

```bash
# Low concurrency (local workers, no network latency)
WORKER_CONCURRENCY=50

# Medium concurrency (LAN, ~10ms latency)
WORKER_CONCURRENCY=200

# High concurrency (wireless LAN, ~50ms latency)
WORKER_CONCURRENCY=500

# Very high concurrency (WAN, >100ms latency)
WORKER_CONCURRENCY=1000
```

### How to Calculate

**Formula:**
```
WORKER_CONCURRENCY = (network_latency_ms / avg_job_time_ms) × desired_throughput_jobs_per_sec / 1000
```

**Example 1: LAN worker, 50ms latency, instant jobs, want 30k jobs/sec**
```
WORKER_CONCURRENCY = (50 / 0.1) × 30000 / 1000
                   = 500 × 30
                   = 15,000

But praktory limits apply, so start with 500-1000
```

**Example 2: LAN worker, 50ms latency, 1ms jobs, want 30k jobs/sec**
```
WORKER_CONCURRENCY = (50 / 1) × 30000 / 1000
                   = 50 × 30
                   = 1,500
```

**Rule of thumb:**
- **Local network (<1ms):** Use 50-100
- **Fast LAN (1-10ms):** Use 100-300
- **Wireless LAN (10-50ms):** Use 300-1000
- **WAN (50-200ms):** Use 1000-2000

## Performance Expectations

### Local Worker (1ms network latency)
```yaml
WORKER_CONCURRENCY=50
```
- **Throughput:** 35,000-50,000 jobs/sec (CPU-bound)
- **Network not a factor**

### LAN Worker (50ms network latency)
```yaml
WORKER_CONCURRENCY=500
```
- **Before (50 workers):** 2,000 jobs/sec (network-bound)
- **After (500 workers):** 20,000-30,000 jobs/sec (CPU-bound on ~20% weaker chip)
- **Expected:** ~28,000-40,000 jobs/sec (80% of local performance)

### How High Concurrency Works

**Network Latency Hiding:**
```
Time: 0ms    50ms   100ms  150ms  200ms
      |      |      |      |      |
Job1: [Fetch]---->[Process]
Job2:   [Fetch]---->[Process]
Job3:     [Fetch]---->[Process]
Job4:       [Fetch]---->[Process]
...
Job500:                [Fetch]---->[Process]

With 500 workers running in parallel:
- While worker 1 is waiting 50ms for fetch
- Workers 2-500 are all processing jobs
- Throughput = jobs_processed / time, not limited by network!
```

## Docker Compose Configuration

### All-in-One (docker-compose.yml)
```yaml
worker-service:
  environment:
    - WORKER_CONCURRENCY=50  # Local, no network latency
```

### Distributed Worker (docker-compose.worker.yml)
```yaml
worker-service:
  environment:
    - WORKER_CONCURRENCY=500  # LAN, hide 50ms latency
```

## Testing

### Test 1: Verify Concurrency Setting
```bash
# Check worker logs
docker-compose -f docker-compose.worker.yml logs worker-service | grep Concurrency

# Should see:
# "Concurrency: 500 jobs per worker"
```

### Test 2: Measure Throughput
```bash
# Submit 10,000 jobs and measure time
time for i in {1..10000}; do
  curl -s -X POST http://api-service:3000/jobs/add \
    -d "{\"a\": $i, \"b\": $i}" &
done
wait

# With WORKER_CONCURRENCY=500, should complete in ~1-2 seconds
# = 5,000-10,000 jobs/sec
```

### Test 3: Monitor Faktory Queue
```
http://faktory:7420

Watch the queue drain rate in real-time
```

## Advanced Tuning

### Memory Considerations

Higher concurrency = more memory:
- Each worker slot holds 1 job in memory
- Estimate: ~1KB per job slot
- 500 workers × 1KB = 500KB (negligible)
- 1000 workers × 1KB = 1MB (still fine)

**Safe limits:**
- Up to 2,000 workers on most systems
- Monitor with `docker stats`

### CPU vs Network Bound

**Check if you're CPU-bound or network-bound:**

```bash
# Run with different concurrency levels
WORKER_CONCURRENCY=100 cargo run --bin worker-service
# Measure jobs/sec

WORKER_CONCURRENCY=500 cargo run --bin worker-service
# Measure jobs/sec

WORKER_CONCURRENCY=1000 cargo run --bin worker-service
# Measure jobs/sec

# If throughput stops increasing, you've hit CPU limit
# If throughput keeps increasing, increase concurrency more
```

### Network Bandwidth

**Typical job size:** ~100 bytes
**At 30,000 jobs/sec:**
- Bandwidth = 30,000 × 100 bytes = 3 MB/sec
- On 100 Mbps network: 3 MB/sec = 24 Mbps (plenty of headroom)
- On 1 Gbps network: 3 MB/sec = 24 Mbps (no problem at all)

**Conclusion:** Network bandwidth is NOT the bottleneck, latency is!

## Faktory Server Configuration

### CPU Limits

Faktory is limited to 2-3 cores by default in docker-compose:

```yaml
faktory:
  deploy:
    resources:
      limits:
        cpus: "2.0"
```

**For high throughput, increase Faktory CPU:**

```yaml
faktory:
  deploy:
    resources:
      limits:
        cpus: "4.0"  # Allow Faktory to use 4 cores
```

This helps Faktory distribute jobs faster to remote workers.

## Complete Configuration Example

### For Your Test (LAN Worker ~20% Weaker Than Local)

**Server (docker-compose.server.yml):**
```yaml
faktory:
  deploy:
    resources:
      limits:
        cpus: "4.0"  # Increase for better distribution

api-service:
  environment:
    - BATCH_MAX_SIZE=100
    - BATCH_MAX_DELAY_MS=50
    - BATCH_AUTO_ENABLED=true
```

**LAN Worker (docker-compose.worker.yml):**
```yaml
worker-service:
  environment:
    - FAKTORY_URL=tcp://${FAKTORY_SERVER_IP}:7419
    - WORKER_CONCURRENCY=1000  # Very high to hide 50ms latency
```

**Expected Results:**
- **Local worker:** 35,000-50,000 jobs/sec (baseline)
- **LAN worker:** 28,000-40,000 jobs/sec (80% of local, limited by CPU not network!)

## Troubleshooting

### Still Seeing Low Throughput

**1. Check concurrency is actually set:**
```bash
docker-compose exec worker-service env | grep WORKER_CONCURRENCY
```

**2. Check if Faktory is the bottleneck:**
```
Visit http://faktory-server:7420
Look at "Busy" count - if it's low, Faktory can't keep up
```

**3. Increase Faktory CPU:**
```yaml
faktory:
  deploy:
    resources:
      limits:
        cpus: "8.0"  # Give Faktory more CPU
```

**4. Check network latency:**
```bash
# From worker machine
ping faktory-server

# Should be <50ms
```

### Memory Usage Too High

Reduce concurrency:
```yaml
- WORKER_CONCURRENCY=300  # Lower value
```

### CPU Usage Too High

You've hit the CPU ceiling - this is good! It means network is no longer the bottleneck.

## Summary

The key insight: **Faktory workers fetch jobs one at a time over the network**. With network latency, you need massive concurrency to keep workers busy while others are waiting on network fetch.

**Changes made:**
1. ✅ Added `WORKER_CONCURRENCY` environment variable
2. ✅ Set default to 500 (high concurrency for network efficiency)  
3. ✅ Updated docker-compose.worker.yml to use 500 for LAN workers
4. ✅ Updated docker-compose.yml to use 50 for local workers

**Expected improvement:**
- From: 2,000 jobs/sec (network-limited)
- To: 28,000-40,000 jobs/sec (CPU-limited, matching local performance!)

Now your LAN worker should perform at near-local speeds, limited only by the CPU being 20% weaker, not by the network! 🚀
