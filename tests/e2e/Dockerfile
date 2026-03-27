FROM rust:latest AS builder
WORKDIR /app
RUN rustup target add x86_64-unknown-linux-musl && \
    apt-get update && apt-get install -y musl-tools
COPY Cargo.toml Cargo.lock CLOUD_HYPERVISOR_VERSION ./
COPY layers layers
COPY bin bin
RUN cargo build --release --bin syfrah --target x86_64-unknown-linux-musl

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    procps \
    iptables \
    jq \
    curl \
    ncat \
    tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/syfrah /usr/local/bin/syfrah

# Install fake cloud-hypervisor for compute E2E scenarios.
# The Python script simulates the CH API socket so that the compute layer
# can be tested end-to-end without KVM.
RUN apt-get update && apt-get install -y python3 && rm -rf /var/lib/apt/lists/*
COPY tests/e2e/fake-cloud-hypervisor.py /usr/local/lib/syfrah/cloud-hypervisor
RUN chmod +x /usr/local/lib/syfrah/cloud-hypervisor

CMD ["sleep", "infinity"]
