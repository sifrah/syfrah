FROM rust:latest AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY layers layers
COPY bin bin
RUN cargo build --release --bin syfrah

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/syfrah /usr/local/bin/syfrah

CMD ["sleep", "infinity"]
