FROM rust:1.93-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin syfrah

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/syfrah /usr/local/bin/syfrah
ENTRYPOINT ["syfrah"]
