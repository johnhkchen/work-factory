# Work Factory - Just Commands
# Quick start:
#   Server node:  just server
#   Worker node:  FAKTORY_SERVER_IP=<server-ip> just worker
#   Test batch:   just test-batch
#   Performance:  just perf-test

# Start central server (Faktory + API + Frontend)
server:
    @echo "🚀 Starting server node..."
    @echo ""
    docker compose -f docker-compose.server.yml up -d --build
    @echo ""
    @echo "✅ Server started!"
    @echo ""
    @echo "📊 Access points:"
    @echo "  Web UI:  http://localhost"
    @echo "  Faktory: http://localhost:7420"
    @echo "  API:     http://localhost:3000"
    @echo ""
    @echo "⚙️  Batching config:"
    @echo "  BATCH_MAX_SIZE=100"
    @echo "  BATCH_MAX_DELAY_MS=100"
    @echo "  BATCH_AUTO_ENABLED=true"
    @echo ""
    @echo "📝 Next step: Run 'just worker' on remote machines"
    @echo "   Set FAKTORY_SERVER_IP to this machine's IP address"

# Start worker node (set FAKTORY_SERVER_IP first)
worker:
    #!/usr/bin/env bash
    if [ -z "$FAKTORY_SERVER_IP" ]; then
        echo "❌ Error: Set FAKTORY_SERVER_IP environment variable"
        echo ""
        echo "Usage:"
        echo "  FAKTORY_SERVER_IP=192.168.1.100 just worker"
        echo ""
        echo "To find your server IP:"
        echo "  hostname -I | awk '{print \$1}'"
        exit 1
    fi
    echo "🔧 Starting worker node..."
    echo ""
    docker compose -f docker-compose.worker.yml up -d --build
    echo ""
    echo "✅ Worker started and connected to $FAKTORY_SERVER_IP"
    echo ""
    echo "⚙️  Worker config:"
    echo "  WORKER_CONCURRENCY=500 (optimized for network latency)"
    echo ""
    echo "📊 Monitor worker:"
    echo "  just worker-logs"
    echo "  just worker-stats"

# Start local development (all services on one machine)
dev:
    docker compose up -d
    @echo "Dev environment started!"
    @echo "Web UI: http://localhost"
    @echo "Faktory: http://localhost:7420"
    @echo "API: http://localhost:3000"

# Stop all services
down:
    docker compose down 2>/dev/null || true
    docker compose -f docker-compose.server.yml down 2>/dev/null || true
    docker compose -f docker-compose.worker.yml down 2>/dev/null || true
    @echo "All services stopped"

# View logs
logs service="":
    #!/usr/bin/env bash
    if [ -n "{{service}}" ]; then
        docker compose logs -f {{service}}
    else
        docker compose logs -f
    fi

# Check service status
status:
    @echo "📊 Service Status:"
    @echo ""
    docker compose ps 2>/dev/null || docker compose -f docker-compose.server.yml ps 2>/dev/null || docker compose -f docker-compose.worker.yml ps
    @echo ""
    @echo "📈 Faktory Stats:"
    @curl -s http://localhost:7420/stats 2>/dev/null | jq '{queue: .faktory.queues.default, processed: .faktory.total_processed, workers: .faktory.tasks.Workers.size}' || echo "Server not available"

# Monitor worker logs
worker-logs:
    docker compose -f docker-compose.worker.yml logs -f worker-service

# Monitor server logs
server-logs service="api-service":
    docker compose -f docker-compose.server.yml logs -f {{service}}

# Show worker stats (jobs/sec)
worker-stats:
    @echo "📊 Worker Performance Stats"
    @echo ""
    docker compose -f docker-compose.worker.yml logs worker-service | grep "Concurrency:" | tail -1 || echo "Worker not running"
    @echo ""
    @echo "Faktory Web UI: http://$(FAKTORY_SERVER_IP:-localhost):7420"

# Test batching system
test-batch:
    @echo "🧪 Testing batching system..."
    @echo ""
    ./test_batching.sh

# Run benchmark
bench:
    cd benchmark && cargo run --release --bin benchmark

# Run large batch enqueue test
bench-large:
    cd benchmark && cargo run --release --bin large

# Quick performance test (submit 1000 jobs)
perf-test jobs="1000":
    #!/usr/bin/env bash
    echo "⚡ Performance Test: Submitting {{jobs}} jobs..."
    echo ""
    START=$(date +%s)
    for i in $(seq 1 {{jobs}}); do
        curl -s -X POST http://localhost:3000/jobs/add \
            -H "Content-Type: application/json" \
            -d "{\"a\": $i, \"b\": $((i*2))}" > /dev/null &
    done
    wait
    END=$(date +%s)
    DURATION=$((END - START))
    JOBS_PER_SEC=$(({{jobs}} / DURATION))
    echo ""
    echo "✅ Submitted {{jobs}} jobs in ${DURATION}s"
    echo "📊 Throughput: ${JOBS_PER_SEC} jobs/sec"
    echo ""
    echo "Check Faktory UI to see jobs being processed:"
    echo "  http://localhost:7420"

# Stress test with batch endpoint (10k jobs in one request)
stress-test:
    #!/usr/bin/env bash
    echo "💪 Stress Test: 10,000 jobs via batch endpoint..."
    echo ""

    # Generate JSON for 10k jobs
    JOBS=$(for i in $(seq 1 10000); do
        echo "{\"type\": \"Add\", \"args\": {\"a\": $i, \"b\": $((i*2))}}"
    done | jq -s '.')

    START=$(date +%s)
    curl -s -X POST http://localhost:3000/jobs/batch \
        -H "Content-Type: application/json" \
        -d "{\"jobs\": $JOBS}" | jq
    END=$(date +%s)
    DURATION=$((END - START))

    echo ""
    echo "✅ Submitted 10,000 jobs in ${DURATION}s"
    echo "📊 Submission rate: $((10000 / DURATION)) jobs/sec"
    echo ""
    echo "Watch processing in Faktory UI:"
    echo "  http://localhost:7420"

# Build all images
build:
    docker compose build

# Clean everything (including volumes)
clean:
    docker compose down -v 2>/dev/null || true
    docker compose -f docker-compose.server.yml down -v 2>/dev/null || true
    docker compose -f docker-compose.worker.yml down -v 2>/dev/null || true
    @echo "All services and volumes removed"

# Show this help
help:
    @just --list
