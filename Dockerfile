FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /data
COPY --from=builder /app/target/release/ptn /usr/local/bin/ptn
EXPOSE 8085
ENTRYPOINT ["ptn"]
CMD ["serve", "--db", "/data/ptnotifier.db", "--bind", "0.0.0.0:8085"]
