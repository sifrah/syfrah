FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    && rm -rf /var/lib/apt/lists/*

COPY syfrah /usr/local/bin/syfrah
RUN chmod +x /usr/local/bin/syfrah

# Keep container running
CMD ["sleep", "infinity"]
