#!/bin/bash

# Test script for the batching system
# This demonstrates both the batch endpoint and auto-batching behavior

API_URL="${API_URL:-http://localhost:3000}"

echo "=== Testing Batching System ==="
echo ""

# Test 1: Batch endpoint
echo "Test 1: Submitting 10 jobs via batch endpoint"
echo "----------------------------------------------"

cat <<EOF | curl -s -X POST "$API_URL/jobs/batch" \
  -H "Content-Type: application/json" \
  -d @- | jq
{
  "jobs": [
    {"type": "Add", "args": {"a": 1, "b": 2}},
    {"type": "Subtract", "args": {"a": 10, "b": 5}},
    {"type": "Multiply", "args": {"a": 3, "b": 4}},
    {"type": "Divide", "args": {"a": 20, "b": 4}},
    {"type": "Add", "args": {"a": 100, "b": 200}},
    {"type": "Subtract", "args": {"a": 50, "b": 25}},
    {"type": "Multiply", "args": {"a": 7, "b": 8}},
    {"type": "Divide", "args": {"a": 100, "b": 10}},
    {"type": "Add", "args": {"a": 5.5, "b": 4.5}},
    {"type": "Multiply", "args": {"a": 2.5, "b": 2}}
  ]
}
EOF

echo ""
echo ""

# Test 2: Individual jobs with auto-batching
echo "Test 2: Submitting individual jobs (will be auto-batched)"
echo "-----------------------------------------------------------"

echo "Sending 5 individual add requests rapidly..."
for i in {1..5}; do
  curl -s -X POST "$API_URL/jobs/add" \
    -H "Content-Type: application/json" \
    -d "{\"a\": $i, \"b\": $(($i * 10))}" | jq -c
done

echo ""
echo ""

# Test 3: Health check
echo "Test 3: Health check"
echo "---------------------"
curl -s "$API_URL/health" | jq

echo ""
echo ""
echo "=== Test Complete ==="
echo ""
echo "Configuration Tips:"
echo "- BATCH_MAX_SIZE: Maximum jobs per batch (default: 100)"
echo "- BATCH_MAX_DELAY_MS: Max time to wait before flushing (default: 50ms)"
echo "- BATCH_AUTO_ENABLED: Enable auto-batching for individual endpoints (default: true)"
echo ""
echo "Example:"
echo "  BATCH_MAX_SIZE=50 BATCH_MAX_DELAY_MS=100 BATCH_AUTO_ENABLED=true ./target/debug/api-service"
