#!/bin/bash
set -e

echo "Running database migrations..."
/usr/local/bin/migration up

echo "Starting application..."
exec "$@"
