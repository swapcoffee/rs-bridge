FROM rust:latest AS builder

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./

RUN cargo new --bin app
WORKDIR /usr/src/app/app

COPY Cargo.toml Cargo.lock ./

COPY src ./src

RUN cargo build --release

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

# Required for reqwest library
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/app/target/release/sse-bridge /usr/local/bin/app

CMD ["app"]
