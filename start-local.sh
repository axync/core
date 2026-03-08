#!/bin/bash

# Start ZKClear API locally with Docker

echo "Starting ZKClear API with Docker..."

# Create data directory if it doesn't exist
mkdir -p ./data

# Start services
docker-compose -f docker-compose.local.yml up -d

echo ""
echo "ZKClear API is starting..."
echo "API will be available at: http://localhost:3000"
echo ""
echo "To view logs: docker-compose -f docker-compose.local.yml logs -f"
echo "To stop: docker-compose -f docker-compose.local.yml down"
echo ""

# Wait a bit and check health
sleep 5
curl -s http://localhost:3000/health | jq . || echo "API is still starting..."

