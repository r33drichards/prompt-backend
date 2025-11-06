# Add webhook endpoint for Railway redeployment

## Summary

This PR adds a webhook endpoint that the IP allocator can call when returning an IP. The endpoint triggers a Railway deployment redeployment to refresh the deployment to a fresh state.

## Changes

- **New endpoint**: `POST /webhook/return` that receives item return notifications
- **Railway integration**: Makes blocking GraphQL request to Railway API to trigger redeployment
- **Configuration**: Added environment variables for Railway API credentials
- **Error handling**: Added `internal_server_error` helper to Error type
- **Version bump**: Bumped Cargo version to 0.2.0 and SDK to 0.1.6

## How It Works

1. IP allocator calls `POST /webhook/return` with payload: `{"item": {...}}`
2. Endpoint reads Railway API credentials from environment
3. Makes blocking GraphQL POST to Railway's API with redeployment mutation
4. Waits for response before returning (blocking behavior)
5. Returns success/failure to IP allocator

## Configuration

### Environment Variables

Add these to your Railway/deployment environment:

```bash
RAILWAY_API_KEY=your_railway_api_key_here
RAILWAY_DEPLOYMENT_ID=your_deployment_id_here
```

### IP Allocator Configuration

Configure this service as a return subscriber in your IP allocator's `config.toml`:

```toml
[return.subscribers.prompt-backend-redeploy]
post = "https://your-prompt-backend-url.railway.app/webhook/return"
mustSucceed = true  # Set to true if redeployment must succeed before returning IP
async = false       # Endpoint blocks until Railway responds
```

#### Configuration Options

- **`post`**: The webhook URL to POST to (your prompt-backend deployment URL + `/webhook/return`)
- **`mustSucceed`**:
  - `true` - Webhook failure prevents the IP from being returned to the freelist
  - `false` - Webhook is best-effort, IP is returned regardless
- **`async`**:
  - `false` - IP allocator waits for webhook to complete (recommended)
  - `true` - IP allocator polls webhook's status endpoint

## Testing

All tests pass:
- ✅ Unit tests
- ✅ OpenAPI spec snapshot updated
- ✅ Code formatted with `cargo fmt`

## Related Documentation

- Railway GraphQL API: https://docs.railway.app/reference/public-api
- Railway Deployment Redeploy mutation: Refreshes deployment to clean state

## Files Changed

- `src/handlers/webhooks.rs` - New webhook handler
- `src/handlers/mod.rs` - Register webhooks module
- `src/main.rs` - Add webhook endpoint to routes
- `src/error.rs` - Add `internal_server_error` helper
- `.env.example` - Document Railway configuration
- `Cargo.toml` - Bump version to 0.2.0
- `sdk/package.json` - Bump SDK version to 0.1.6
