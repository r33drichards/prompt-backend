FROM rust:1.84 AS builder

WORKDIR /usr/src/app

COPY . .

RUN cargo build --release
RUN cd migration && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/rust-redis-webserver /usr/local/bin/rust-redis-webserver
COPY --from=builder /usr/src/app/target/release/migration /usr/local/bin/migration
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 8000

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["rust-redis-webserver"]
