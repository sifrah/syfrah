#!/usr/bin/env bash
# Download assets required for compute E2E tests (KVM).
#
# This script downloads a minimal Linux kernel and Alpine Linux rootfs
# from Cloud Hypervisor's test asset repository. These are needed to run
# the KVM-based E2E tests in layers/compute/tests/e2e_kvm.rs.
#
# Usage:
#   ./scripts/setup-compute-e2e.sh
#
# Assets are placed in /tmp/syfrah-e2e-assets/ by default.
# Override with: ASSET_DIR=/path/to/dir ./scripts/setup-compute-e2e.sh

set -euo pipefail

ASSET_DIR="${ASSET_DIR:-/tmp/syfrah-e2e-assets}"

# Cloud Hypervisor v43.0 test assets
CH_VERSION="v43.0"
BASE_URL="https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/${CH_VERSION}"

# Hypervisor firmware / kernel — use the CH-provided test kernel
KERNEL_URL="https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/0.4.2/hypervisor-fw"
KERNEL_SHA256="d8c4b3ebe9d459b3a9e2aae382da61bd847ca1e4f7899e8a2b41ebce12eee7bb"
KERNEL_FILE="hypervisor-fw"

# Alpine Linux minimal rootfs (Cloud Hypervisor test image)
ROOTFS_URL="https://cloud-hypervisor.azureedge.net/focal-server-cloudimg-amd64-custom-20210106-0.raw.gz"
ROOTFS_SHA256="a2116d79978a0c1b7ef3cb2f9c3df324e06cfbab5c9f55e0d35ef6b2b8dca534"
ROOTFS_FILE="rootfs.raw.gz"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

info()  { echo -e "${YELLOW}=> $1${NC}"; }
ok()    { echo -e "${GREEN}=> $1${NC}"; }
err()   { echo -e "${RED}=> $1${NC}"; }

echo -e "${BOLD}Syfrah Compute E2E — Asset Setup${NC}"
echo ""

# ── Check prerequisites ──────────────────────────────────────────

if ! command -v curl &>/dev/null; then
    err "curl is required but not found"
    exit 1
fi

if ! command -v sha256sum &>/dev/null; then
    err "sha256sum is required but not found"
    exit 1
fi

if [ ! -e /dev/kvm ]; then
    echo -e "${YELLOW}WARNING: /dev/kvm not found. KVM E2E tests require a KVM-capable host.${NC}"
    echo ""
fi

# ── Create asset directory ───────────────────────────────────────

mkdir -p "$ASSET_DIR"
info "Asset directory: $ASSET_DIR"
echo ""

# ── Download kernel ──────────────────────────────────────────────

download_asset() {
    local url="$1"
    local file="$2"
    local expected_sha="$3"
    local dest="$ASSET_DIR/$file"

    if [ -f "$dest" ]; then
        info "$file already exists, verifying checksum..."
        actual_sha=$(sha256sum "$dest" | awk '{print $1}')
        if [ "$actual_sha" = "$expected_sha" ]; then
            ok "$file checksum OK (skipping download)"
            return 0
        else
            info "$file checksum mismatch, re-downloading..."
        fi
    fi

    info "Downloading $file..."
    curl -fSL --progress-bar -o "$dest" "$url"

    actual_sha=$(sha256sum "$dest" | awk '{print $1}')
    if [ "$actual_sha" = "$expected_sha" ]; then
        ok "$file checksum OK"
    else
        err "$file checksum FAILED"
        err "  expected: $expected_sha"
        err "  actual:   $actual_sha"
        rm -f "$dest"
        exit 1
    fi
}

download_asset "$KERNEL_URL" "$KERNEL_FILE" "$KERNEL_SHA256"
download_asset "$ROOTFS_URL" "$ROOTFS_FILE" "$ROOTFS_SHA256"

# ── Decompress rootfs if needed ──────────────────────────────────

if [ -f "$ASSET_DIR/$ROOTFS_FILE" ] && [ ! -f "$ASSET_DIR/rootfs.raw" ]; then
    info "Decompressing rootfs..."
    gunzip -k "$ASSET_DIR/$ROOTFS_FILE"
    mv "$ASSET_DIR/focal-server-cloudimg-amd64-custom-20210106-0.raw" "$ASSET_DIR/rootfs.raw" 2>/dev/null || \
        mv "$ASSET_DIR/${ROOTFS_FILE%.gz}" "$ASSET_DIR/rootfs.raw" 2>/dev/null || true
    ok "Rootfs decompressed"
fi

# ── Summary ──────────────────────────────────────────────────────

echo ""
echo -e "${BOLD}Setup complete.${NC}"
echo ""
echo "Assets in $ASSET_DIR:"
ls -lh "$ASSET_DIR/"
echo ""
echo -e "${BOLD}To run KVM E2E tests:${NC}"
echo ""
echo "  export SYFRAH_E2E_KERNEL=$ASSET_DIR/$KERNEL_FILE"
echo "  export SYFRAH_E2E_ROOTFS=$ASSET_DIR/rootfs.raw"
echo "  cargo test -p syfrah-compute -- --ignored"
echo ""
echo "Requirements:"
echo "  - KVM-capable host (/dev/kvm must exist)"
echo "  - cloud-hypervisor binary installed"
echo "  - Root privileges"
