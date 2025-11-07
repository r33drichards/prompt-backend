# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

This is a Rust web service built with Rocket, providing a session management API with OAuth authentication via Keycloak. The system uses PostgreSQL for persistent storage, Redis for job queues, and includes background task processing with Apalis.

## Development Commands

### Running the Application

```bash
# Run web server only
cargo run -- --server

# Run all background tasks
cargo run -- --all-bg-tasks

# Run specific background tasks
cargo run -- --bg-tasks outbox-publisher

# Run multiple specific background tasks
cargo run -- --bg-tasks outbox-publisher ip-return-poller

# Run both web server and all background tasks
cargo run -- --server --all-bg-tasks

# Print OpenAPI specification
cargo run -- print-openapi
```

### Development with Nix

```bash
# Enter development environment
nix develop

# Build Docker image with Nix
nix build .#docker

# Load Docker image
nix run .#loadDockerImage
```

### Docker Compose

```bash
# Start all services (PostgreSQL, Redis, Keycloak, web server, Prometheus, Grafana)
docker compose up --build

# Stop services
docker compose down

# Stop and remove volumes
docker compose down -v
```

### Database Operations

```bash
# Run migrations (handled automatically on server start)
cargo run -- --server

# Access database via Docker
docker exec -it prompt-backend-postgres psql -U promptuser -d prompt_backend

# Access database directly
psql postgres://promptuser:promptpass@localhost:5432/prompt_backend
```

### Testing and Quality

```bash
# Run unit tests
cargo test

# Run unit tests (via Nix)
nix develop --command cargo test --all-targets

# Check formatting
cargo fmt -- --check

# Run clippy
cargo clippy -- -D warnings

# Snapshot testing (uses insta crate)
cargo test
```

### TypeScript SDK Generation

```bash
# Generate TypeScript client (requires Nix)
nix run .#generateTypescriptClient

# The SDK is automatically published to npm via GitHub Actions:
# - Main branch: @wholelottahoopla/prompt-backend-client@<version>
# - PRs: @wholelottahoopla/prompt-backend-client@<version>-beta.pr<number>.<sha>
```

## Development Workflow

### Before Completing Tasks

**IMPORTANT**: Before finishing any task that involves code changes, always run:

```bash
# Format code
cargo fmt

# Run clippy and fix any warnings
cargo clippy -- -D warnings
```

These checks run automatically in CI/CD, so running them locally prevents build failures. Always ensure:
- Code is properly formatted with `cargo fmt`
- No clippy warnings exist
- Tests pass with `cargo test`

## Architecture

### Core Components

- **Web Server** (`src/main.rs`, `src/handlers/`): Rocket-based REST API with automatic OpenAPI documentation
- **Authentication** (`src/auth/`): JWT validation via Keycloak OAuth 2.0
  - `guard.rs`: Request guard for protected endpoints
  - `jwks.rs`: JWKS cache for token validation
- **Database Layer** (`src/db.rs`, `src/entities/`): SeaORM models and database connections
- **Background Tasks** (`src/bg_tasks/`): Apalis-based async job processing
  - `outbox_publisher.rs`: Publishes from PostgreSQL outbox to downstream systems
  - `session_poller.rs`: Polls for new sessions and enqueues processing jobs
- **Services** (`src/services/`): External API integrations (e.g., Anthropic for title generation)

### Request Flow

1. **Protected endpoints**: Request → OAuth guard (`AuthenticatedUser`) → JWT validation → Handler
2. **Session creation**: Client POST → Create handler → Store in PostgreSQL → Outbox entry → Background job
3. **Background processing**: Session poller → Detect new sessions → Enqueue job → Outbox publisher → Process

### Database Schema

Sessions table (`src/entities/session.rs`):
- `id`: UUID primary key
- `user_id`: String, extracted from JWT
- `messages`: JSONB, conversation history
- `inbox_status`: Enum (Pending, Processing, Completed, etc.)
- `session_status`: Enum (Active, Completed, Failed)
- `sbx_config`: JSONB, sandbox configuration
- `repo`, `target_branch`, `branch`: Git-related fields
- `title`: Auto-generated via Anthropic API
- Timestamps: `created_at`, `updated_at`, `deleted_at`

### Background Tasks

The system uses Apalis with PostgreSQL storage for reliable job processing:
- Jobs survive restarts
- Automatic retries on failure
- Prometheus metrics exposed at `/metrics`
- Tasks registered in `src/bg_tasks/mod.rs` via `OUTBOX_PUBLISHER` constant

## Deployment

### CI/CD Pipeline (`.github/workflows/ci-cd.yml`)

The deployment process runs on pushes to `master`:

1. **Test**: Runs unit tests, formatting checks, and clippy
2. **Build**: Uses Nix to build Docker image
3. **Deploy**: Pushes to Docker Hub with SHA and `latest` tags
4. **Deploy-Railway**: Triggers Railway redeployment via GraphQL API

To deploy:
```bash
git add .
git commit -m "Your changes"
git push origin master
# Monitor GitHub Actions at: https://github.com/<org>/<repo>/actions
```

### Environment Variables

Required for all deployments:
- `DATABASE_URL`: PostgreSQL connection string
- `REDIS_URL`: Redis connection string (default: `redis://127.0.0.1/`)
- `KEYCLOAK_ISSUER`: OAuth issuer URL
- `KEYCLOAK_JWKS_URI`: JWKS endpoint for JWT validation
- `ANTHROPIC_API_KEY`: For title generation

Optional:
- `ROCKET_PORT`: Web server port (default: 8000)
- `ROCKET_ADDRESS`: Bind address (default: 0.0.0.0)

See `.env.example` for reference values.

## Code Patterns

### Adding a New Endpoint

1. Add handler function in `src/handlers/<module>.rs`:
```rust
#[openapi]
#[get("/my-endpoint")]
pub async fn my_handler(
    db: &State<DatabaseConnection>,
    _user: AuthenticatedUser,  // Require authentication
) -> OResult<MyOutput> {
    // Implementation
}
```

2. Register in `src/main.rs`:
```rust
openapi_get_routes![
    // ... existing routes
    handlers::my_module::my_handler,
]
```

3. Add to OpenAPI spec generation in `generate_openapi_spec()` function

### Adding a New Background Task

1. Create module in `src/bg_tasks/<task_name>.rs`
2. Define job struct and handler function
3. Add constant in `src/bg_tasks/mod.rs`: `pub const MY_TASK: &str = "my-task";`
4. Update `all_tasks()` function to include new task
5. Add registration logic in `TaskContext::register_task()`

### Database Migrations

Migrations live in `migration/src/`:
- Follow naming: `m<YYYYMMDD>_<sequence>_<description>.rs`
- Run automatically on server startup via `migration::Migrator::up()`
- Create with: Add file, implement `Migration` trait, register in `migration/src/lib.rs`

## API Documentation

- **Swagger UI**: http://localhost:8000/swagger-ui/
- **RapiDoc**: http://localhost:8000/rapidoc/
- **Metrics**: http://localhost:8000/metrics (Prometheus format)
- **Health Check**: http://localhost:8000/health (no authentication)

Protected endpoints require `Authorization: Bearer <jwt-token>` header.

## Monitoring

The system includes Prometheus and Grafana via Docker Compose:
- **Prometheus**: http://localhost:9090
- **Grafana**: http://localhost:3000 (admin/admin)
- Metrics include:
  - Background job success/failure rates
  - Job duration histograms
  - Queue depths
  - Custom application metrics

## Local Development Setup

See `SETUP_LOCAL.md` for detailed local development instructions.
See `QUICK_REFERENCE.md` for quick access to URLs, credentials, and commands.
