{ pkgs
, rustToolchain
}:

let
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "sandbox-client";
  version = "0.1.0";

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    openssl
  ];

  # The build will read openapi.json during the build process
  # via build.rs, so it needs to be available
  preBuild = ''
    ls -la
    echo "Checking openapi.json..."
    head -5 openapi.json
  '';

  meta = with pkgs.lib; {
    description = "Rust SDK for the Sandbox API";
    homepage = "https://github.com/your-repo/agent-sandbox-sdk";
    license = licenses.mit;
  };
}
