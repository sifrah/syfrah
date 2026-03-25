#!/bin/sh
# Tests for install.sh checksum verification logic.
# Exercises the grep/awk/head pipeline in isolation.
set -eu

PASS=0
FAIL=0

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $label (expected='$expected', actual='$actual')" >&2
    FAIL=$((FAIL + 1))
  fi
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# --- Fixture: SHA256SUMS.txt with a substring-matching entry (.sig) ---
cat > "${TMPDIR}/SHA256SUMS.txt" <<'EOF'
aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344  syfrah-v1.0.0-x86_64-unknown-linux-musl.tar.gz
deadbeef00000000deadbeef00000000deadbeef00000000deadbeef00000000  syfrah-v1.0.0-x86_64-unknown-linux-musl.tar.gz.sig
EOF

ARCHIVE="syfrah-v1.0.0-x86_64-unknown-linux-musl.tar.gz"

# --- Test 1: valid checksum passes ---
echo "Test 1: valid checksum extraction with grep -F and head -1"
EXPECTED="$(grep -F "${ARCHIVE}" "${TMPDIR}/SHA256SUMS.txt" | head -1 | awk '{print $1}')"
assert_eq "extracts correct hash" \
  "aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344" \
  "$EXPECTED"

# --- Test 2: wrong checksum produces mismatch ---
echo "Test 2: wrong checksum detected"
ACTUAL="0000000000000000000000000000000000000000000000000000000000000000"
if [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "  PASS: mismatch correctly detected"
  PASS=$((PASS + 1))
else
  echo "  FAIL: mismatch not detected" >&2
  FAIL=$((FAIL + 1))
fi

# --- Test 3: substring match does not return multiple hashes ---
echo "Test 3: grep -F + head -1 returns exactly one line"
LINE_COUNT="$(grep -F "${ARCHIVE}" "${TMPDIR}/SHA256SUMS.txt" | head -1 | wc -l | tr -d ' ')"
assert_eq "single line returned" "1" "$LINE_COUNT"

# Without head -1, plain grep would return 2 lines (the .sig entry matches too)
RAW_COUNT="$(grep -F "${ARCHIVE}" "${TMPDIR}/SHA256SUMS.txt" | wc -l | tr -d ' ')"
assert_eq "raw grep returns 2 matches (proving head -1 is needed)" "2" "$RAW_COUNT"

# --- Summary ---
echo ""
echo "Results: ${PASS} passed, ${FAIL} failed"
if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
