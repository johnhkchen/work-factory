# Work Factory - Just Commands

# Start central server (Faktory + API + Frontend)
server:
    docker compose -f docker-compose.server.yml up -d
    @echo "Server started!"
    @echo "Web UI: http://localhost"
    @echo "Faktory: http://localhost:7420"
    @echo "API: http://localhost:3000"

# Start worker node (set FAKTORY_SERVER_IP first)
worker:
    #!/usr/bin/env bash
    if [ -z "$FAKTORY_SERVER_IP" ]; then
        echo "Error: Set FAKTORY_SERVER_IP environment variable"
        echo "Usage: FAKTORY_SERVER_IP=192.168.1.100 just worker"
        exit 1
    fi
    docker compose -f docker-compose.worker.yml up -d
    echo "Worker connected to $FAKTORY_SERVER_IP"

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
    docker compose ps
    @echo ""
    @curl -s http://localhost:7420/stats 2>/dev/null | jq '{queue: .faktory.queues.default, processed: .faktory.total_processed, workers: .faktory.tasks.Workers.size}' || echo "Server not running"

# Run benchmark
bench:
    cd benchmark && cargo run --release --bin benchmark

# Run large batch enqueue test
bench-large:
    cd benchmark && cargo run --release --bin large

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
