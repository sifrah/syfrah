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
# Install Python for the fake cloud-hypervisor and gcc for compiling the wrapper.
RUN apt-get update && apt-get install -y python3 gcc && rm -rf /var/lib/apt/lists/*
COPY tests/e2e/fake-cloud-hypervisor.py /usr/local/lib/syfrah/fake-ch.py

# Build a minimal C wrapper that execs the Python script.
# This avoids issues with shebang handling from musl static binaries.
# Build a minimal C wrapper that execs python3 -u with the fake-ch script.
# The -u flag makes Python output unbuffered for reliable log capture.
RUN printf '#include <stdio.h>\n#include <unistd.h>\n#include <string.h>\n#include <errno.h>\nint main(int argc, char *argv[]) {\n  char *args[64]; int i;\n  args[0]="/usr/bin/python3"; args[1]="-u"; args[2]="/usr/local/lib/syfrah/fake-ch.py";\n  for(i=1;i<argc&&i<60;i++) args[i+2]=argv[i];\n  args[i+2]=0;\n  execv("/usr/bin/python3",args);\n  fprintf(stderr,"execv failed: %%s\\n",strerror(errno));\n  return 1;\n}\n' > /tmp/w.c && \
    gcc -o /usr/local/lib/syfrah/cloud-hypervisor /tmp/w.c && rm /tmp/w.c

# Create stub kernel and image files for compute preflight checks.
# These are never actually booted — the fake-cloud-hypervisor.py handles
# all CH API calls — but the preflight validator requires them to exist.
RUN mkdir -p /opt/syfrah/images /run/syfrah/vms && \
    touch /opt/syfrah/vmlinux && \
    touch /opt/syfrah/images/alpine-3.20.raw

CMD ["sleep", "infinity"]
