# Session Processing Fix

## Problem Summary

Sessions created via the API were not being enqueued and processed in production.

## Root Cause Analysis

### Architecture Overview

The system has a 3-part architecture:

1. **Session Creation** (src/handlers/sessions.rs:110-169)
   - API endpoint creates sessions with `inbox_status: Active`
   - Does NOT directly enqueue jobs

2. **Session Poller** (src/bg_tasks/session_poller.rs)
   - Runs in a loop every 1 second
   - Queries for sessions with `inbox_status == Active` and `session_status == Active`
   - Enqueues OutboxJob for each active session
   - Updates session to `inbox_status: Pending`

3. **Outbox Publisher** (src/bg_tasks/outbox_publisher.rs)
   - Background worker that processes OutboxJob items
   - Handles actual session processing (IP allocation, repo cloning, running Claude Code)

### The Bug

The session poller and outbox publisher are ONLY started when the application runs with the `--bg-tasks` flag (see src/main.rs:121-136).

In docker-compose.yml, the webserver was configured as:
```yaml
command: ["rust-redis-webserver", "--server"]
```

**Missing: `--bg-tasks` flag!**

This meant:
- API server was running and creating sessions
- Session poller was NOT running
- Sessions stayed in Active status forever
- No processing occurred

## The Fix

Added a separate `background-worker` service in docker-compose.yml that runs with `--bg-tasks --all` flag.

### Changes Made

1. Added new service in docker-compose.yml:
   ```yaml
   background-worker:
     image: rust-redis-webserver:latest
     command: ["rust-redis-webserver", "--bg-tasks", "--all"]
     depends_on:
       postgres:
         condition: service_healthy
       redis:
         condition: service_started
       keycloak:
         condition: service_healthy
     environment:
       # All necessary environment variables for background processing
       ...
   ```

2. Environment variables required for background worker:
   - DATABASE_URL - for database access
   - KEYCLOAK_URL - for Keycloak admin API
   - KEYCLOAK_REALM - realm name
   - KEYCLOAK_ADMIN_USERNAME - admin username
   - KEYCLOAK_ADMIN_PASSWORD - admin password
   - IP_ALLOCATOR_URL - for IP allocation service

## Verification Steps

### 1. Build and Deploy

```bash
# Build the Docker image
docker build -t rust-redis-webserver:latest .

# Start all services
docker-compose up -d
```

### 2. Check Logs

Verify the background worker is running:

```bash
# Check background-worker logs
docker-compose logs background-worker

# You should see:
# "Starting background tasks: [\"outbox-publisher\"]"
# "Starting session poller - checking every 1 second"
# "Worker [worker-id] started"
```

### 3. Test Session Processing

1. Create a session via API:
   ```bash
   curl -X POST http://localhost:8000/sessions \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer YOUR_TOKEN" \
     -d '{
       "repo": "owner/repo",
       "target_branch": "main"
     }'
   ```

2. Check the database - session should transition from `Active` to `Pending`:
   ```bash
   docker exec -it prompt-backend-postgres psql -U promptuser -d prompt_backend -c \
     "SELECT id, inbox_status, session_status FROM session ORDER BY created_at DESC LIMIT 5;"
   ```

3. Monitor background-worker logs:
   ```bash
   docker-compose logs -f background-worker

   # You should see:
   # "Enqueued 1 active sessions for processing"
   # "Processing outbox job for session_id: <uuid>"
   # "Borrowed sandbox - mcp_json_string: ..."
   # "Running Claude Code CLI for session <uuid>"
   ```

### 4. Check Apalis Job Queue

```bash
docker exec -it prompt-backend-postgres psql -U promptuser -d prompt_backend -c \
  "SELECT id, job_type, status, done_at FROM apalis.jobs ORDER BY run_at DESC LIMIT 10;"
```

Jobs should show up and transition to completed status.

## Testing in Development

To run locally with background workers:

```bash
# Terminal 1 - Run server
cargo run -- --server

# Terminal 2 - Run background tasks
cargo run -- --bg-tasks --all
```

Or run both in one terminal:

```bash
cargo run -- --server --bg-tasks --all
```

## Key Insights

1. The session poller is a critical component that bridges API requests and background processing
2. Production deployments need BOTH webserver AND background worker services
3. The `--bg-tasks` flag must be present for any job processing to occur
4. Without the poller, sessions accumulate in the database without processing

## Future Improvements

Consider:
1. Adding health checks for the background worker
2. Adding metrics/monitoring for session processing
3. Adding alerts for when sessions are stuck in Active status
4. Consider alternative architectures (e.g., enqueue directly in API handler instead of polling)
