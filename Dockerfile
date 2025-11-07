FROM rust:1.84 AS builder

WORKDIR /usr/src/app

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/rust-redis-webserver /usr/local/bin/rust-redis-webserver

EXPOSE 8000

CMD ["rust-redis-webserver", "--server"]
