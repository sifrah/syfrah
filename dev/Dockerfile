FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    procps \
    && rm -rf /var/lib/apt/lists/*

# syfrah binary is volume-mounted at /usr/local/bin/syfrah
# No ENTRYPOINT: containers stay alive via tail -f /dev/null
CMD ["tail", "-f", "/dev/null"]
