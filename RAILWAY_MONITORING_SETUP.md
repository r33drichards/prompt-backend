# Railway Monitoring Setup Guide

This guide will help you deploy Prometheus and Grafana on Railway to monitor your prompt-backend service.

## Problem Identified

Your `prompt-backend` service was not processing queued sessions because the Dockerfile CMD was missing the required flags. The fix has been applied:

**Before:**
```dockerfile
CMD ["rust-redis-webserver"]
```

**After:**
```dockerfile
CMD ["rust-redis-webserver", "--server", "--bg-tasks", "-A"]
```

This ensures both the web server and background task processors (including the outbox publisher) are running.

## Architecture

The system now works as follows:

1. **Session Creation** → Session is created with `inbox_status=Active`, `session_status=Active` (handlers/sessions.rs:147-154)
2. **Session Poller** → Polls every 1 second for Active sessions, pushes them to the Apalis queue, marks as Pending (bg_tasks/session_poller.rs)
3. **Outbox Publisher Worker** → Processes jobs from the queue (bg_tasks/outbox_publisher.rs)

## Metrics Available

The application now exposes Prometheus metrics at `http://<your-service-url>/metrics`:

- **Apalis Worker Metrics:**
  - `apalis_jobs_total` - Total number of jobs processed
  - `apalis_jobs_duration_seconds` - Job processing duration
  - `apalis_jobs_failed_total` - Failed jobs count
  - `apalis_active_jobs` - Currently active jobs

## Railway Deployment Steps

### 1. Redeploy Your prompt-backend Service

First, push your changes and redeploy:

```bash
git add .
git commit -m "Fix: Add --server and --bg-tasks flags to Dockerfile CMD and Prometheus metrics"
git push
```

Railway will automatically redeploy your service with the updated Dockerfile.

### 2. Deploy Prometheus on Railway

Create a new service in your Railway project:

1. Go to your Railway project
2. Click "New Service" → "Empty Service"
3. Name it "prometheus"
4. Add the following environment variables:
   - `RAILWAY_STATIC_URL` (Railway will generate this)
5. In the service settings:
   - **Deploy from Docker Image**: `prom/prometheus:latest`
   - **Port**: `9090`

6. Create a custom `prometheus.yml` config (you'll need to mount this):

Since Railway doesn't easily support volume mounts for config files, you have two options:

#### Option A: Use Environment Variable for Config

Create a custom Prometheus Dockerfile:

```dockerfile
FROM prom/prometheus:latest

COPY prometheus.yml /etc/prometheus/prometheus.yml

EXPOSE 9090

ENTRYPOINT ["/bin/prometheus"]
CMD ["--config.file=/etc/prometheus/prometheus.yml", \
     "--storage.tsdb.path=/prometheus", \
     "--web.console.libraries=/usr/share/prometheus/console_libraries", \
     "--web.console.templates=/usr/share/prometheus/consoles"]
```

Update `prometheus.yml` to use your Railway service URL:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'prompt-backend'
    static_configs:
      - targets: ['prompt-backend-production.up.railway.app:443']
    scheme: https
    metrics_path: '/metrics'
    scrape_interval: 5s
```

#### Option B: Use Grafana Cloud (Recommended for Railway)

Since Railway services can be ephemeral and managing Prometheus state can be tricky, consider using Grafana Cloud's free tier:

1. Sign up for Grafana Cloud: https://grafana.com/products/cloud/
2. Get your Prometheus remote write endpoint
3. Configure your app to push metrics directly (using `prometheus-push-gateway` or similar)

### 3. Deploy Grafana on Railway

Create another new service:

1. Click "New Service" → "Empty Service"
2. Name it "grafana"
3. Deploy from Docker Image: `grafana/grafana:latest`
4. Port: `3000`
5. Add environment variables:
   - `GF_SECURITY_ADMIN_USER=admin`
   - `GF_SECURITY_ADMIN_PASSWORD=<your-secure-password>`
   - `GF_SERVER_ROOT_URL=https://<your-grafana-url>.up.railway.app`

### 4. Configure Grafana Data Source

1. Access your Grafana instance at `https://<your-grafana-url>.up.railway.app`
2. Login with the admin credentials
3. Go to Configuration → Data Sources → Add data source
4. Select "Prometheus"
5. Set the URL to your Prometheus service: `http://prometheus.railway.internal:9090` (if using Railway's private networking)
6. Click "Save & Test"

### 5. Create Dashboards

Import the Apalis dashboard or create a custom one:

**Key Metrics to Monitor:**

1. **Job Processing Rate**
   ```promql
   rate(apalis_jobs_total[5m])
   ```

2. **Failed Jobs**
   ```promql
   apalis_jobs_failed_total
   ```

3. **Job Duration**
   ```promql
   histogram_quantile(0.95, rate(apalis_jobs_duration_seconds_bucket[5m]))
   ```

4. **Active Jobs**
   ```promql
   apalis_active_jobs
   ```

## Alternative: Simple Monitoring Without Prometheus

If you want to quickly check metrics without setting up full monitoring:

```bash
# Check metrics endpoint
curl https://<your-service-url>.up.railway.app/metrics
```

## Debugging Tips

### 1. Check if background tasks are running

Look at your Railway logs for:
```
Starting background tasks: ["outbox-publisher"]
Starting session poller - checking every 1 second
Worker [<id>] started
```

### 2. Check database for sessions

Connect to your Railway PostgreSQL and run:
```sql
SELECT id, inbox_status, session_status, created_at
FROM session
ORDER BY created_at DESC
LIMIT 10;
```

### 3. Monitor job queue

Check the Apalis queue table:
```sql
SELECT * FROM apalis.jobs
ORDER BY run_at DESC
LIMIT 10;
```

### 4. Check metrics endpoint

```bash
curl https://<your-service-url>.up.railway.app/metrics | grep apalis
```

## Local Testing with Docker Compose

For local development, you can use the updated `docker-compose.yml`:

```bash
# Build and start all services including Prometheus and Grafana
docker-compose up --build

# Access services:
# - Backend: http://localhost:8000
# - Prometheus: http://localhost:9090
# - Grafana: http://localhost:3000 (admin/admin)
```

## Next Steps

1. **Redeploy** your prompt-backend service with the updated Dockerfile
2. **Test** by creating a new session and checking if it gets processed
3. **Monitor** the `/metrics` endpoint to see if metrics are being collected
4. **Set up alerting** in Grafana for failed jobs or slow processing times

## Troubleshooting

**Sessions still not processing?**
- Check Railway logs for the "Starting background tasks" message
- Verify DATABASE_URL environment variable is set correctly
- Check for any errors in the worker logs

**Metrics not showing up?**
- Verify the `/metrics` endpoint is accessible
- Check Prometheus is scraping successfully (Status → Targets in Prometheus UI)
- Ensure Apalis PrometheusLayer is properly initialized

**Questions?**
Check the code references:
- Main entry point: `src/main.rs:64-150`
- Background tasks: `src/bg_tasks/mod.rs:40-127`
- Outbox publisher: `src/bg_tasks/outbox_publisher.rs`
- Session poller: `src/bg_tasks/session_poller.rs`
