# Rust + Redis + Rocket Webserver Template

A production-ready template for building Rust web services with Redis storage, featuring:

- 🚀 **Rocket** web framework with automatic OpenAPI documentation
- 📦 **Redis** for data persistence with an abstract store interface
- 🗄️ **PostgreSQL** for relational data with SeaORM
- ⚙️ **Apalis** background job processing with Redis and PostgreSQL backends
- 🐳 **Docker** and Docker Compose for containerized deployment
- ❄️ **Nix Flake** for reproducible development environments
- ✅ **E2E tests** with GitHub Actions CI/CD
- 📚 **Swagger UI** and **RapiDoc** for interactive API documentation

## Features

### Abstract Store Interface

The template provides a clean abstraction over Redis with basic CRUD operations:

- **Create**: Add new items to the store
- **Read**: Retrieve and remove items from the store
- **List**: View all items without removing them
- **Update**: Modify existing items
- **Delete**: Remove specific items

This abstraction makes it easy to:
- Swap storage backends (Redis → PostgreSQL, etc.)
- Add business logic without touching infrastructure code
- Test with mock stores

### Automatic OpenAPI Documentation

All endpoints automatically generate OpenAPI specs thanks to `rocket_okapi`:
- Swagger UI available at `/swagger-ui/`
- RapiDoc available at `/rapidoc/`
- Export OpenAPI JSON with `cargo run -- --print-openapi`

## Quick Start

### Option 1: Using Nix (Recommended)

```bash
# Clone the template
git clone <your-template-url>
cd rust-redis-webserver-template

# Enter the development environment
nix develop

# Start Redis
redis-server &

# Run the webserver
cargo run

# Visit the API documentation
open http://localhost:8000/swagger-ui/
```

### Option 2: Using Docker Compose

```bash
# Build and start all services
docker compose up --build

# Visit the API documentation
open http://localhost:8000/swagger-ui/
```

### Option 3: Manual Setup

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt-get install build-essential pkg-config libssl-dev redis-server

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Start Redis
redis-server &

# Run the webserver
cargo run
```

## API Endpoints

All endpoints accept and return JSON.

### Create Item
```bash
POST /items
Content-Type: application/json

{
  "item": {
    "name": "example",
    "value": 42
  }
}
```

### List Items
```bash
GET /items
```

### Read Item (Pop)
```bash
GET /items/read
```

### Update Item
```bash
PUT /items
Content-Type: application/json

{
  "old_item": {"name": "example", "value": 42},
  "new_item": {"name": "example", "value": 100}
}
```

### Delete Item
```bash
DELETE /items
Content-Type: application/json

{
  "item": {"name": "example", "value": 42}
}
```

## Running Tests

### E2E Tests

```bash
# Run the complete CRUD test suite
./scripts/test_crud.sh
```

The test script:
1. Builds Docker images
2. Starts Redis and the webserver
3. Runs comprehensive CRUD tests
4. Verifies each operation
5. Cleans up containers

### Unit Tests

```bash
cargo test
```

## Development

### Project Structure

```
.
├── src/
│   ├── main.rs              # Application entry point
│   ├── store.rs             # Abstract store interface (Redis)
│   ├── error.rs             # Error handling
│   └── handlers/
│       ├── mod.rs
│       └── items.rs         # CRUD endpoint handlers
├── scripts/
│   └── test_crud.sh         # E2E test script
├── .github/
│   └── workflows/
│       └── integration-tests.yml  # CI/CD pipeline
├── Cargo.toml               # Rust dependencies
├── flake.nix                # Nix development environment
├── docker-compose.yml       # Docker orchestration
└── Dockerfile               # Multi-stage build
```

### Extending the Template

#### Adding New Endpoints

1. Add a new handler in `src/handlers/`:
```rust
#[openapi]
#[get("/health")]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok".into() })
}
```

2. Register it in `src/main.rs`:
```rust
.mount(
    "/",
    openapi_get_routes![
        // ... existing routes
        handlers::health::health,
    ],
)
```

#### Replacing Redis with Another Database

The `Store` trait in `src/store.rs` provides a clean abstraction. To swap backends:

1. Keep the same interface (create, read, list, update, delete)
2. Implement the methods for your new database
3. Update `Cargo.toml` dependencies
4. Update `docker-compose.yml` if needed

Example for PostgreSQL:
```rust
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub async fn create(&self, item: &Value) -> Result<()> {
        // PostgreSQL implementation
    }
    // ... other methods
}
```

#### Adding Authentication

Add a guard in `src/guards/`:
```rust
pub struct ApiKey(String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey {
    // ... implementation
}
```

Then use it in handlers:
```rust
#[post("/items", data = "<input>")]
pub async fn create(
    _key: ApiKey,  // <- Authentication guard
    store: &State<Mutex<Store>>,
    input: Json<CreateInput>,
) -> OResult<CreateOutput> {
    // ...
}
```

## Background Tasks

The application includes an Apalis-based background job processing system with two task types:

### Available Tasks

1. **outbox-publisher**: Reads from PostgreSQL outbox table and publishes to Redis
2. **session-handler**: Reads from Redis and processes session data

### Running Background Tasks

```bash
# Run all background tasks
cargo run -- --bg-tasks -A

# Run specific tasks
cargo run -- --bg-tasks outbox-publisher session-handler

# Run only background tasks (no web server)
cargo run -- --bg-tasks outbox-publisher

# Run web server with background tasks
cargo run -- serve --bg-tasks session-handler

# Run web server only (default)
cargo run
```

### CLI Options

- `serve`: Run the web server (default if no bg-tasks specified)
- `--bg-tasks <TASKS>`: Enable background tasks
  - `-A` or `--all`: Run all available tasks
  - Or specify task names: `outbox-publisher session-handler`
- `print-openapi`: Print OpenAPI specification and exit

### Task Implementations

Background tasks are located in `src/bg_tasks/`:

- `outbox_publisher.rs`: Handles outbox pattern for reliable message publishing
- `session_handler.rs`: Processes session-related jobs from Redis queue

Each task can be customized by implementing the job handler function and registering it with the monitor.

## Configuration

### Environment Variables

- `REDIS_URL`: Redis connection string (default: `redis://127.0.0.1/`)
- `DATABASE_URL`: PostgreSQL connection string (required)
- `ROCKET_ADDRESS`: Server bind address (default: `0.0.0.0`)
- `ROCKET_PORT`: Server port (default: `8000`)

### Using a .env File

```bash
# Create .env file
cat > .env <<EOF
REDIS_URL=redis://localhost:6379/
ROCKET_PORT=3000
EOF

# The application will automatically load it
cargo run
```

## Deployment

### Docker

```bash
# Build the image
docker build -t my-webserver .

# Run with Redis
docker run -d --name redis redis:alpine
docker run -d --name webserver \
  -p 8000:8000 \
  -e REDIS_URL=redis://redis:6379/ \
  --link redis \
  my-webserver
```

### Docker Compose (Production)

```bash
docker compose up -d
```

### Nix

```bash
# Build the package
nix build

# Run the built binary
./result/bin/rust-redis-webserver
```

## CI/CD

The template includes GitHub Actions workflows:

- **Integration Tests**: Runs on every push and PR
  - Builds Docker images
  - Runs E2E test suite
  - Reports failures with container logs

To use in your repository:
1. Push to GitHub
2. GitHub Actions will automatically run tests
3. Check the "Actions" tab for results

## License

This template is provided as-is for use in your projects.

## Acknowledgments

Based on patterns from [ip-allocator-webserver](https://github.com/r33drichards/ip-allocator-webserver), stripped of business logic to create a reusable template.
