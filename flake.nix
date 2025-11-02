{
  description = "Rust + Redis + Rocket Webserver Template";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Use the specific Rust toolchain required by Rocket
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Build inputs for the Rust project
        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
          redis
        ];

        # Script to generate TypeScript API client
        generateTypescriptClientScript = pkgs.writeShellScriptBin "generate-typescript-client" ''
          set -e

          echo "🔨 Building the backend..."
          ${pkgs.cargo}/bin/cargo build --release

          echo "📝 Generating OpenAPI specification..."
          OPENAPI_SPEC=$(${pkgs.cargo}/bin/cargo run --release print-openapi)

          # Create a temporary directory for the spec
          TEMP_DIR=$(mktemp -d)
          echo "$OPENAPI_SPEC" > "$TEMP_DIR/openapi.json"

          echo "🚀 Generating TypeScript client..."
          OUTPUT_DIR="''${1:-./generated-client}"
          rm -rf "$OUTPUT_DIR"

          ${pkgs.openapi-generator-cli}/bin/openapi-generator-cli generate \
            -i "$TEMP_DIR/openapi.json" \
            -g typescript-fetch \
            -o "$OUTPUT_DIR" \
            --additional-properties=npmName=@r33drichards/prompt-backend-client,npmVersion=0.1.0,supportsES6=true,typescriptThreePlus=true

          echo "📦 Setting up npm package..."
          cd "$OUTPUT_DIR"

          # Add additional package.json metadata if needed
          ${pkgs.jq}/bin/jq '.description = "TypeScript API client for prompt-backend" | .author = "r33drichards" | .license = "MIT"' package.json > package.json.tmp
          mv package.json.tmp package.json

          # Install dependencies
          ${pkgs.nodejs}/bin/npm install

          # Build the TypeScript client
          ${pkgs.nodejs}/bin/npm run build || echo "No build script found, skipping..."

          echo "✅ TypeScript client generated successfully in $OUTPUT_DIR"
          echo ""
          echo "To publish to npm:"
          echo "  cd $OUTPUT_DIR"
          echo "  npm login"
          echo "  npm publish --access public"

          # Cleanup
          rm -rf "$TEMP_DIR"
        '';

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

          packages = with pkgs; [
            # Development tools
            cargo-watch
            cargo-edit

            # Docker tools
            docker
            docker-compose

            # Testing and debugging
            curl
            jq

            # Redis CLI for debugging
            redis

            # OpenAPI and TypeScript client generation
            openapi-generator-cli
            nodejs
          ];

          shellHook = ''
            echo "🦀 Rust + Redis + Rocket Development Environment"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build          - Build the project"
            echo "  cargo run            - Run the webserver"
            echo "  cargo test           - Run unit tests"
            echo "  docker compose up    - Start services (Redis + Webserver)"
            echo "  ./scripts/test_crud.sh - Run e2e tests"
            echo ""
            echo "OpenAPI Documentation:"
            echo "  Swagger UI: http://localhost:8000/swagger-ui/"
            echo "  RapiDoc:    http://localhost:8000/rapidoc/"
            echo ""
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          REDIS_URL = "redis://127.0.0.1:6379/";
        };

        # Package the Rust application
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rust-redis-webserver";
          version = "0.1.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = nativeBuildInputs;
          buildInputs = buildInputs;

          meta = with pkgs.lib; {
            description = "A Rust webserver with Redis CRUD operations";
            license = licenses.mit;
          };
        };

        # Docker image (optional, uses Dockerfile)
        packages.docker = pkgs.dockerTools.buildLayeredImage {
          name = "rust-redis-webserver";
          tag = "latest";
          contents = [ self.packages.${system}.default ];
          config = {
            Cmd = [ "${self.packages.${system}.default}/bin/rust-redis-webserver" ];
            ExposedPorts = {
              "8000/tcp" = {};
            };
          };
        };

        # Apps
        apps.generateTypescriptClient = {
          type = "app";
          program = "${generateTypescriptClientScript}/bin/generate-typescript-client";
        };
      }
    );
}
