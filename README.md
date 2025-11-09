# Work Factory

A distributed job queue system built with Rust and Faktory.

## Architecture

Designed for **1 central server + 4 worker machines**:

- **Server node**: Runs Faktory, API, and Web UI
- **Worker nodes**: Run job processors, connect to central Faktory

## Quick Start

### Local Development (Single Machine)

```bash
just dev              # Start all services
just status           # Check status
just bench            # Run benchmark
just down             # Stop services
```

### Distributed Setup (Production)

**On server machine:**
```bash
just server
```

**On each of 4 worker machines:**
```bash
export FAKTORY_SERVER_IP=192.168.1.100  # Your server IP
just worker
```

## Available Commands

```bash
just dev          # Start local dev environment
just server       # Start central server
just worker       # Start worker (requires FAKTORY_SERVER_IP)
just status       # Show service status and queue stats
just logs [svc]   # View logs (optional service name)
just bench        # Run throughput benchmark
just bench-large  # Enqueue 2M jobs for testing
just build        # Build all Docker images
just down         # Stop all services
just clean        # Stop and remove all volumes
```

## Configuration

- **Faktory**: 2 cores (handles 4 remote workers efficiently)
- **Workers**: 1 core each, 30 concurrent jobs
- **Connection pool**: 50 connections per worker
- **Nginx**: High-throughput settings, no rate limiting

## Performance

Optimized for distributed deployment with minimal resource usage per node.

## Tech Stack

Rust + Faktory 1.9.3 + Axum 0.8.6 + HTMX + Nginx
