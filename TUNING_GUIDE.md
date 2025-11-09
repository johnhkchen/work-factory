# Performance Tuning Guide

## Current Performance: 16k jobs/sec with CPU Headroom

If you're seeing 16k jobs/sec but still have available CPU, you can push higher! Here's how to tune the system to maximize throughput.

## Key Tuning Parameters

### 1. Worker Concurrency (WORKER_CONCURRENCY)

**What it does:** Number of concurrent job slots in the worker. Each slot can fetch and process one job at a time.

**Why it matters:** With network latency, you need many slots so that while some workers are waiting on network fetch, others are processing jobs.

**Current setting:** 2000 (default in worker node)

**How to tune:**
```bash
# Start with 2000
FAKTORY_SERVER_IP=192.168.1.100 just worker 2000

# Increase until CPU maxes out or performance stops improving
FAKTORY_SERVER_IP=192.168.1.100 just worker 3000
FAKTORY_SERVER_IP=192.168.1.100 just worker 4000
FAKTORY_SERVER_IP=192.168.1.100 just worker 5000
```

**Monitoring:**
```bash
# Watch CPU usage
docker stats

# If CPU < 90%, increase concurrency
# If CPU = 100%, you've hit the ceiling!
```

### 2. Worker CPU Limit (WORKER_CPUS)

**What it does:** Maximum CPU cores the worker container can use.

**Current setting:** 4.0 cores (default)

**How to tune:**
```bash
# Match your hardware
# 4-core machine:
FAKTORY_SERVER_IP=192.168.1.100 just worker 3000 4.0

# 8-core machine:
FAKTORY_SERVER_IP=192.168.1.100 just worker 5000 8.0

# 16-core machine:
FAKTORY_SERVER_IP=192.168.1.100 just worker 8000 16.0
```

**Rule of thumb:**
- Concurrency ≈ 500-1000 per core
- More cores = more concurrency needed

### 3. Faktory CPU Limit

**What it does:** Faktory server CPU allocation for job distribution.

**Current setting:** 8.0 cores (in docker-compose.server.yml)

**How to tune:**

If Faktory is the bottleneck (check with `docker stats`), increase in `docker-compose.server.yml`:

```yaml
faktory:
  deploy:
    resources:
      limits:
        cpus: "16.0"  # Give Faktory more cores
```

### 4. Batch Size (BATCH_MAX_SIZE)

**What it does:** Jobs per batch from client → Faktory.

**Current setting:** 100

**How to tune:**

For very high throughput, increase batch size:

```yaml
api-service:
  environment:
    - BATCH_MAX_SIZE=500  # Larger batches
    - BATCH_MAX_DELAY_MS=200  # Higher timeout
```

## Tuning Workflow

### Step 1: Baseline Test
```bash
# Start worker with defaults
FAKTORY_SERVER_IP=192.168.1.100 just worker

# Run performance test
just perf-test 10000

# Note: jobs/sec and CPU usage
```

### Step 2: Increase Concurrency
```bash
# Try doubling concurrency
just down  # Stop current worker
FAKTORY_SERVER_IP=192.168.1.100 just worker 4000

# Test again
just perf-test 10000

# Compare results
```

### Step 3: Monitor Resources
```bash
# Watch CPU, memory, network in real-time
docker stats

# Check Faktory queue depth
# http://server-ip:7420

# If queue is growing → Faktory bottleneck
# If queue is empty but CPU < 100% → Increase concurrency
# If CPU = 100% → You've maxed out!
```

### Step 4: Adjust CPU Limits
```bash
# If hitting CPU limit at < 100% system CPU
just down
FAKTORY_SERVER_IP=192.168.1.100 just worker 5000 8.0

# Test again
just perf-test 10000
```

## Example Tuning Session

```bash
# Test 1: Default (2000 concurrency, 4 cores)
FAKTORY_SERVER_IP=192.168.1.100 just worker
just perf-test 10000
# Result: 16k jobs/sec, 60% CPU
# → Increase concurrency

# Test 2: Higher concurrency (4000, 4 cores)
just down
FAKTORY_SERVER_IP=192.168.1.100 just worker 4000
just perf-test 10000
# Result: 24k jobs/sec, 90% CPU
# → Getting close, try more

# Test 3: Even higher (6000, 4 cores)
just down
FAKTORY_SERVER_IP=192.168.1.100 just worker 6000
just perf-test 10000
# Result: 28k jobs/sec, 100% CPU
# → Maxed out! This is optimal

# Final: Use worker with 6000 concurrency
FAKTORY_SERVER_IP=192.168.1.100 just worker 6000
```

## Quick Reference

### Environment Variables

```bash
# Worker node
export FAKTORY_SERVER_IP=192.168.1.100
export WORKER_CONCURRENCY=6000
export WORKER_CPUS=8.0
just worker
```

### Just Commands with Parameters

```bash
# Default (2000 concurrency, 4 cores)
just worker

# Custom concurrency
just worker 3000

# Custom concurrency + CPU
just worker 5000 8.0
```

### Monitoring Commands

```bash
# Real-time stats
docker stats

# Worker logs
just worker-logs

# Faktory UI
http://server-ip:7420

# Performance test
just perf-test 10000
```

## Expected Results by Hardware

### 4-Core Worker (Your Current Test)
- **Concurrency:** 4000-6000
- **Throughput:** 25k-35k jobs/sec
- **CPU:** 100%

### 8-Core Worker
- **Concurrency:** 8000-12000
- **Throughput:** 50k-70k jobs/sec
- **CPU:** 100%

### 16-Core Worker
- **Concurrency:** 16000-24000
- **Throughput:** 100k-140k jobs/sec
- **CPU:** 100%

## Troubleshooting

### High concurrency but low throughput
**Symptom:** Set WORKER_CONCURRENCY=5000 but only getting 10k jobs/sec

**Possible causes:**
1. Faktory bottleneck - Check `docker stats` on server
2. Network bandwidth - Check with `iftop` or `nethogs`
3. Job processing too slow - Profile your job handlers

**Solutions:**
```bash
# Increase Faktory CPU
# Edit docker-compose.server.yml:
faktory:
  deploy:
    resources:
      limits:
        cpus: "16.0"

# Restart server
just down
just server
```

### Memory usage too high
**Symptom:** Worker using excessive memory

**Solution:** Reduce concurrency or add memory limit:
```yaml
worker-service:
  deploy:
    resources:
      limits:
        cpus: "8.0"
        memory: "4G"  # Add memory limit
```

### Network saturated
**Symptom:** Network at 100% but CPU low

**Solution:** You've hit network bandwidth limit. This is rare but possible with very small jobs.

**Check:**
```bash
# Monitor network
iftop  # or nethogs

# If network maxed:
# - Use larger batch sizes
# - Reduce job submission rate
# - Upgrade network infrastructure
```

## Best Practices

1. **Start conservative, increase gradually**
   - Begin with default 2000 concurrency
   - Double until you see diminishing returns

2. **Monitor everything**
   - CPU usage (docker stats)
   - Queue depth (Faktory UI)
   - Network bandwidth (iftop)

3. **Match hardware**
   - More cores = higher concurrency
   - Rule: 500-1000 concurrency per core

4. **Test under realistic load**
   - Use `just perf-test` with job counts matching production
   - Run sustained tests, not just bursts

5. **Document your optimal settings**
   - Once tuned, record the values
   - Set them as defaults in docker-compose.worker.yml

## Summary

To push beyond 16k jobs/sec with CPU headroom:

1. **Increase worker concurrency:** `just worker 4000` → `just worker 6000`
2. **Monitor CPU:** Watch `docker stats` until CPU hits 100%
3. **Adjust CPU limits if needed:** `just worker 6000 8.0`
4. **Test:** `just perf-test 10000` and measure throughput

**Goal:** CPU at 100%, throughput maximized!

When you hit 100% CPU and performance stops improving, you've found the network ceiling. That's your maximum distributed throughput! 🚀
