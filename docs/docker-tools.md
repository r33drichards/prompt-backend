# Docker Tools Usage

This project includes Nix-based Docker image building using `pkgs.dockerTools`.

## Available Docker Packages

### `packages.docker`

Builds a compressed Docker image tarball using `buildLayeredImage`. This creates a `.tar.gz` file in the Nix store that can be loaded into Docker.

**Usage:**
```bash
# Build the image
nix build .#docker

# Load into Docker
docker image load -i result
```

### `packages.dockerStream`

Creates a **script** that streams a Docker image when executed using `streamLayeredImage`. This is more efficient for CI and direct loading as it doesn't save the image to the Nix store.

**Usage:**
```bash
# Build the stream script
nix build .#dockerStream

# Stream directly into Docker
./result | docker image load
```

## Apps

### `apps.loadDockerImage`

A convenient `nix run` command that builds and loads the Docker image in one step.

**Usage:**
```bash
# Stream and load the image into Docker daemon in one command
nix run .#loadDockerImage
```

This will:
1. Build the image layers
2. Stream them directly to Docker
3. Display usage instructions

**Output:**
```
🐳 Streaming Docker image into Docker daemon...
Loaded image: rust-redis-webserver:latest
✅ Image loaded successfully!

Run with:
  docker run --rm -p 8000:8000 rust-redis-webserver:latest
```

## CI/CD Integration

For CI pipelines, use `streamLayeredImage` to avoid storing large image tarballs:

```yaml
# GitHub Actions example
- name: Build and push Docker image
  run: |
    nix build .#dockerStream
    ./result | docker image load
    docker push rust-redis-webserver:latest
```

## Benefits of streamLayeredImage

1. **Space Efficient**: Doesn't store compressed tarball in Nix store
2. **Faster CI**: Streams directly to Docker, reducing IO operations
3. **Deduplication**: Layers are automatically deduplicated based on Nix store paths
4. **Reproducible**: Same Nix input = same Docker image output

## Image Configuration

Both images include:
- The Rust webserver binary
- CA certificates for SSL/TLS
- Exposed port 8000
- Environment variable for SSL certificates

The image uses layered builds, where each dependency gets its own layer for better caching and smaller updates.
