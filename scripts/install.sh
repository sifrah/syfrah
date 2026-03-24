#!/bin/sh
# Install script for syfrah — downloads the latest release binary.
set -eu

REPO="sifrah/syfrah"
BIN="syfrah"
INSTALL_DIR="/usr/local/bin"

# --- Detect OS ---
OS="$(uname -s)"
case "$OS" in
  Linux)  OS="unknown-linux-musl" ;;
  Darwin) OS="apple-darwin" ;;
  *)      echo "Error: unsupported operating system: $OS" >&2; exit 1 ;;
esac

# --- Detect architecture ---
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64)             ARCH="x86_64" ;;
  aarch64|arm64)      ARCH="aarch64" ;;
  *)                  echo "Error: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"

# --- Fetch latest release tag ---
echo "Fetching latest release version..."
VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')"

if [ -z "$VERSION" ]; then
  echo "Error: could not determine latest release version" >&2
  exit 1
fi

echo "Latest version: ${VERSION}"

# --- Download archive ---
ARCHIVE="${BIN}-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${URL}..."
curl -fSL -o "${TMPDIR}/${ARCHIVE}" "$URL"

# --- Extract and install ---
echo "Extracting ${ARCHIVE}..."
tar xzf "${TMPDIR}/${ARCHIVE}" -C "$TMPDIR"

echo "Installing ${BIN} to ${INSTALL_DIR}..."
install -m 755 "${TMPDIR}/${BIN}" "${INSTALL_DIR}/${BIN}"

# --- Verify ---
echo "Verifying installation..."
EXPECTED_VERSION="${VERSION#v}"
if command -v "$BIN" > /dev/null 2>&1; then
  ACTUAL_VERSION=$("$BIN" --version | awk '{print $2}')
  if [ "$ACTUAL_VERSION" != "$EXPECTED_VERSION" ]; then
    echo "ERROR: version mismatch — expected ${EXPECTED_VERSION}, got ${ACTUAL_VERSION}" >&2
    exit 1
  fi
  echo "${BIN} ${ACTUAL_VERSION}"
  echo "Installation complete."
else
  echo "Warning: ${BIN} was installed to ${INSTALL_DIR} but is not on PATH." >&2
  echo "Add ${INSTALL_DIR} to your PATH, then run: ${BIN} --version" >&2
fi
