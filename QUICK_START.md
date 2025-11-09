# Quick Start Guide

Get your distributed job processing system running in 2 minutes!

## Prerequisites

- Docker & Docker Compose installed
- `just` command runner ([install](https://github.com/casey/just#installation))
- Network connectivity between nodes

## Step 1: Start Server Node

On your main server machine:

```bash
# Clone the repository
git clone <your-repo>
cd work-factory

# Start the server
just server
```

You'll see:
```
✅ Server started!

📊 Access points:
  Web UI:  http://localhost
  Faktory: http://localhost:7420
  API:     http://localhost:3000

⚙️  Batching config:
  BATCH_MAX_SIZE=100
  BATCH_MAX_DELAY_MS=100
  BATCH_AUTO_ENABLED=true
```

**Note the server's IP address:**
```bash
hostname -I | awk '{print $1}'
# Example output: 192.168.1.100
```

## Step 2: Start Worker Node(s)

On your LAN/remote worker machine(s):

```bash
# Clone the repository
git clone <your-repo>
cd work-factory

# Start worker (replace with your server IP)
FAKTORY_SERVER_IP=192.168.1.100 just worker
```

You'll see:
```
✅ Worker started and connected to 192.168.1.100

⚙️  Worker config:
  WORKER_CONCURRENCY=500 (optimized for network latency)
```

## Step 3: Test the System

On the server machine:

```bash
# Quick test - submit 1000 jobs
just perf-test 1000

# Expected output:
# ✅ Submitted 1000 jobs in 2s
# 📊 Throughput: 500 jobs/sec
```

Or run the batching test:

```bash
# Test batch endpoint
just test-batch
```

## Step 4: Monitor Performance

### Faktory Web UI
```
http://localhost:7420
```

Watch jobs being processed in real-time!

### Worker Logs
```bash
just worker-logs
```

### Server Logs
```bash
just server-logs
```

### System Status
```bash
just status
```

## Performance Testing

### Small Test (1,000 jobs)
```bash
just perf-test 1000
```

### Medium Test (10,000 jobs)
```bash
just perf-test 10000
```

### Stress Test (10,000 jobs via batch endpoint)
```bash
just stress-test
```

## Expected Performance

### Local Network
- **Throughput:** 35,000-50,000 jobs/sec
- **Limited by:** CPU

### Wireless LAN (50ms latency)
- **Throughput:** 20,000-40,000 jobs/sec
- **Limited by:** CPU (not network!)
- **Improvement over non-batched:** 200x

## Troubleshooting

### Worker can't connect to server
```bash
# Check connectivity from worker machine
ping 192.168.1.100
telnet 192.168.1.100 7419

# Check firewall on server
sudo ufw allow 7419/tcp  # Faktory
sudo ufw allow 3000/tcp  # API (optional)
```

### No jobs being processed
```bash
# Check worker is running
just worker-stats

# Check server logs
just server-logs

# Check Faktory UI
# http://<server-ip>:7420
```

### Low performance
```bash
# Check worker concurrency
docker compose -f docker-compose.worker.yml exec worker-service env | grep WORKER_CONCURRENCY
# Should show: WORKER_CONCURRENCY=500

# Increase if needed
# Edit docker-compose.worker.yml:
#   - WORKER_CONCURRENCY=1000
```

## Common Just Commands

```bash
just server          # Start server node
just worker          # Start worker node
just dev             # Start all services locally
just down            # Stop all services
just status          # Show service status
just test-batch      # Test batching system
just perf-test       # Performance test
just stress-test     # Stress test
just worker-logs     # Watch worker logs
just server-logs     # Watch server logs
just clean           # Stop and remove everything
just --list          # Show all commands
```

## Clean Shutdown

```bash
# On worker nodes
just down

# On server node
just down
```

## Architecture

```
┌─────────────────┐
│  Server Node    │
│  - Faktory      │
│  - API          │
│  - Frontend     │
└────────┬────────┘
         │
    ┌────┴────┬────────┬────────┐
    │         │        │        │
┌───▼───┐ ┌───▼───┐ ┌──▼────┐ ┌──▼────┐
│Worker1│ │Worker2│ │Worker3│ │WorkerN│
│ (LAN) │ │ (LAN) │ │ (WAN) │ │ (...)  │
└───────┘ └───────┘ └───────┘ └───────┘
```

## What's Happening

1. **Client** submits jobs to API
2. **API** batches jobs (100 per batch, 100ms max wait)
3. **Faktory** distributes jobs to workers
4. **Workers** fetch and process jobs (500 concurrent slots)
5. **Batching + High Concurrency** hides network latency

**Result:** LAN workers perform at near-local speeds! 🚀

## Next Steps

- Read [BATCHING_GUIDE.md](BATCHING_GUIDE.md) for detailed configuration
- Read [NETWORK_PERFORMANCE_TUNING.md](NETWORK_PERFORMANCE_TUNING.md) for optimization
- Read [DOCKER_DEPLOYMENT.md](DOCKER_DEPLOYMENT.md) for advanced deployment

## One-Line Setup

**Server:**
```bash
git clone <repo> && cd work-factory && just server
```

**Worker:**
```bash
git clone <repo> && cd work-factory && FAKTORY_SERVER_IP=192.168.1.100 just worker
```

That's it! You're processing jobs at 20k-40k jobs/sec across the network! 🎉
