FROM rust:latest AS builder
WORKDIR /app
RUN rustup target add x86_64-unknown-linux-musl && \
    apt-get update && apt-get install -y musl-tools
COPY Cargo.toml Cargo.lock ./
COPY layers layers
COPY bin bin
RUN cargo build --release --bin syfrah --target x86_64-unknown-linux-musl

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/syfrah /usr/local/bin/syfrah

CMD ["sleep", "infinity"]
