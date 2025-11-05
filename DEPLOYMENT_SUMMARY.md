# Deployment Summary

## Root Cause Identified ✅

Your sessions weren't being processed because the Dockerfile CMD was missing the required flags to start both the web server and background task processors.

**The Fix:**
```dockerfile
# Before (Dockerfile:17)
CMD ["rust-redis-webserver"]

# After (Dockerfile:17)
CMD ["rust-redis-webserver", "--server", "--bg-tasks", "-A"]
```

## Changes Deployed

### 1. Core Fix - Background Task Processing
- **File:** `Dockerfile`
- **Change:** Added `--server --bg-tasks -A` flags to CMD
- **Impact:** Both web server and background workers now run together
- **Commit:** `58f1f8f`

### 2. Prometheus Metrics Integration
- **New Files:**
  - `src/handlers/metrics.rs` - Metrics endpoint handler
  - `prometheus.yml` - Prometheus scrape configuration
  - `prometheus-railway.yml` - Railway-specific Prometheus config
  - `grafana-datasources.yml` - Grafana datasource provisioning

- **Modified Files:**
  - `Cargo.toml` - Added Prometheus dependencies
  - `src/bg_tasks/mod.rs` - Added PrometheusLayer to workers
  - `src/main.rs` - Registered metrics endpoint
  - `src/handlers/mod.rs` - Exported metrics module
  - `docker-compose.yml` - Added Prometheus and Grafana services

### 3. Railway Services Created
- ✅ **prometheus** - Metrics collection service
- ✅ **grafana** - Metrics visualization dashboard

## Current Status

### ✅ Working
1. **Session Poller** - Running and querying for active sessions every 1 second
2. **Background Tasks** - Outbox publisher worker is registered and running
3. **Database Queries** - Successfully polling for sessions with `inbox_status=Active` and `session_status=Active`
4. **Git Push** - Code pushed to master branch
5. **Railway Deployment** - Build triggered successfully

### ⏳ In Progress
1. **Deployment Build** - Rust compilation takes ~5-10 minutes on Railway
2. **Metrics Endpoint** - Will be available at `/metrics` once new deployment is live

### 📋 Next Steps (Manual)
1. Wait for Railway deployment to complete (~5-10 minutes)
2. Verify metrics endpoint: `curl https://prompt-backend-production.up.railway.app/metrics`
3. Configure Prometheus to scrape prompt-backend
4. Set up Grafana dashboards

## Available Metrics (Once Deployed)

The `/metrics` endpoint will expose:

```
# Apalis Worker Metrics
apalis_jobs_total - Total number of jobs processed
apalis_jobs_duration_seconds - Job processing duration histogram
apalis_jobs_failed_total - Failed jobs counter
apalis_active_jobs - Currently active jobs gauge

# HTTP metrics (from Rocket)
http_requests_total
http_request_duration_seconds
```

## How the System Works Now

```
1. User creates session
   ↓
2. Session saved with inbox_status=Active, session_status=Active
   (handlers/sessions.rs:147-154)
   ↓
3. Session Poller (runs every 1 second)
   - Queries for Active sessions (bg_tasks/session_poller.rs:42-46)
   - Pushes job to Apalis queue (bg_tasks/session_poller.rs:57-60)
   - Marks session as Pending (bg_tasks/session_poller.rs:63)
   ↓
4. Outbox Publisher Worker
   - Picks up job from queue
   - Processes session (bg_tasks/outbox_publisher.rs:29-328)
   - Sets up sandbox, clones repo, runs Claude Code
```

## Testing the Fix

### 1. Check if background tasks are running

```bash
railway logs --service prompt-backend | grep "Starting background tasks"
```

Expected output:
```
INFO Starting background tasks: ["outbox-publisher"]
INFO Starting session poller - checking every 1 second
INFO Worker [<id>] started
```

### 2. Create a test session

```bash
curl -X POST https://prompt-backend-production.up.railway.app/sessions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d '{
    "repo": "owner/repo",
    "target_branch": "main"
  }'
```

### 3. Monitor processing

```bash
# Watch the logs for job processing
railway logs --service prompt-backend --tail 50

# Expected to see:
# - "Processing outbox job for session_id: <id>"
# - "Borrowed sandbox - mcp_json_string: ..."
# - "Running Claude Code CLI for session <id>"
```

### 4. Check metrics (once deployed)

```bash
curl https://prompt-backend-production.up.railway.app/metrics | grep apalis
```

## Prometheus Configuration

To configure Prometheus to scrape your backend:

1. **Option A:** Update the `prometheus-railway.yml` with your actual service URL
2. **Option B:** Add Prometheus configuration via Railway dashboard:
   - Environment Variables for the prometheus service
   - Mount the config file (requires custom Dockerfile)

## Grafana Setup

1. Access: `https://grafana-<your-id>.up.railway.app`
2. Login: admin/admin (set in environment variables)
3. Add Prometheus datasource:
   - URL: `http://prometheus.railway.internal:9090` (using Railway's private network)
   - Or: `https://prometheus-<your-id>.up.railway.app`
4. Import dashboard or create custom queries

## Useful PromQL Queries

```promql
# Job processing rate
rate(apalis_jobs_total[5m])

# Failed jobs
sum(apalis_jobs_failed_total)

# 95th percentile job duration
histogram_quantile(0.95, rate(apalis_jobs_duration_seconds_bucket[5m]))

# Active jobs
sum(apalis_active_jobs)

# Jobs by status
sum by (status) (apalis_jobs_total)
```

## Troubleshooting

### Sessions still not processing?
1. Check Railway logs for "Starting background tasks" message
2. Verify DATABASE_URL is set correctly
3. Look for worker errors in logs
4. Check if sessions exist in database with Active status

### Metrics endpoint returns 404?
1. Wait for deployment to complete (check Railway dashboard)
2. Verify the build succeeded
3. Check logs for "No matching routes" errors

### Prometheus not scraping?
1. Verify Prometheus can reach the backend service
2. Check Prometheus targets page: `/targets`
3. Ensure the scrape config has correct URL

## Files Changed

```
Modified:
- Dockerfile (CMD updated)
- Cargo.toml (added prometheus deps)
- src/main.rs (registered metrics endpoint)
- src/bg_tasks/mod.rs (added PrometheusLayer)
- src/handlers/mod.rs (exported metrics module)
- docker-compose.yml (added monitoring services)

Created:
- src/handlers/metrics.rs (metrics endpoint)
- prometheus.yml (local config)
- prometheus-railway.yml (Railway config)
- Dockerfile.prometheus (custom Prometheus image)
- grafana-datasources.yml (Grafana provisioning)
- RAILWAY_MONITORING_SETUP.md (detailed setup guide)
- DEPLOYMENT_SUMMARY.md (this file)
```

## Code References

- Dockerfile CMD: `Dockerfile:17`
- Main entry point: `src/main.rs:64-150`
- Background task registration: `src/bg_tasks/mod.rs:80-127`
- Session poller: `src/bg_tasks/session_poller.rs:15-84`
- Outbox publisher: `src/bg_tasks/outbox_publisher.rs:29-328`
- Metrics endpoint: `src/handlers/metrics.rs:10-21`
- Metrics registration: `src/main.rs:221`

## Next Manual Steps

1. **Wait for deployment** (~5-10 min) - Monitor at: https://railway.com/project/c8ceaa84-c222-4842-988a-5eb04440443b/service/eb78de67-f81d-4d16-b767-802acf609d66

2. **Verify metrics endpoint:**
   ```bash
   curl https://prompt-backend-production.up.railway.app/metrics
   ```

3. **Configure Prometheus scraping:**
   - Update prometheus-railway.yml with correct target URL
   - Deploy custom Prometheus image or configure via Railway

4. **Set up Grafana:**
   - Access Grafana service
   - Configure Prometheus datasource
   - Create dashboards for monitoring

5. **Test session processing:**
   - Create a new session via API
   - Watch logs for processing
   - Verify session status changes

## Support

For more details, see:
- `RAILWAY_MONITORING_SETUP.md` - Comprehensive monitoring setup guide
- Railway logs: `railway logs --service prompt-backend`
- Git commit: `58f1f8f`
