FROM rust:latest AS builder
WORKDIR /app
RUN rustup target add x86_64-unknown-linux-musl && \
    apt-get update && apt-get install -y musl-tools
COPY Cargo.toml Cargo.lock CLOUD_HYPERVISOR_VERSION ./
COPY layers layers
COPY bin bin
RUN cargo build --release --bin syfrah --target x86_64-unknown-linux-musl

# Build the fake cloud-hypervisor (Rust static binary, same musl target)
COPY tests/e2e/fake-ch tests/e2e/fake-ch
RUN cargo build --release --manifest-path tests/e2e/fake-ch/Cargo.toml --target x86_64-unknown-linux-musl

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
COPY --from=builder /app/tests/e2e/fake-ch/target/x86_64-unknown-linux-musl/release/fake-cloud-hypervisor /usr/local/lib/syfrah/cloud-hypervisor

# No pre-installed images — cold start tests pull from the real catalog at
# test time via `syfrah compute image pull`. Only the fake CH binary is
# baked in (needed for VM process simulation).
RUN mkdir -p /opt/syfrah/images /run/syfrah/vms

CMD ["sleep", "infinity"]
