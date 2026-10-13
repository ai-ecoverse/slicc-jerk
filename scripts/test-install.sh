#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ASSET_DIR="${1:?usage: test-install.sh <asset-dir>}"
ASSET_DIR="$(cd "$ASSET_DIR" && pwd)"

FAKE_VERSION="${FAKE_VERSION:-0.99.0}"

pass=0
fail=0

assert_contains() {
  case "$1" in
    *"$2"*) pass=$((pass + 1)) ;;
    *)
      printf 'FAIL: output does not contain "%s" (%s)\n' "$2" "${3:-}" >&2
      printf '  got: %s\n' "$1" >&2
      fail=$((fail + 1))
      ;;
  esac
}

assert_fail() {
  if [ "$1" -ne 0 ]; then
    pass=$((pass + 1))
  else
    printf 'FAIL: expected non-zero exit (%s)\n' "${2:-}" >&2
    fail=$((fail + 1))
  fi
}

assert_ok() {
  if [ "$1" -eq 0 ]; then
    pass=$((pass + 1))
  else
    printf 'FAIL: expected zero exit (%s)\n' "${2:-}" >&2
    fail=$((fail + 1))
  fi
}

WORK_DIR="$(mktemp -d)"

SERVE_ROOT="$WORK_DIR/serve"
mkdir -p "$SERVE_ROOT/latest/download"

OS_NAME="$(uname -s)"
case "$OS_NAME" in
  Darwin) OS_LOWER=darwin ;;
  Linux)  OS_LOWER=linux ;;
  *)      printf 'unsupported OS for test: %s\n' "$OS_NAME" >&2; exit 1 ;;
esac
UARCH="$(uname -m)"
case "$UARCH" in
  x86_64|amd64)  ARCH_LOWER=amd64 ;;
  arm64|aarch64)  ARCH_LOWER=arm64 ;;
  *)              printf 'unsupported arch for test: %s\n' "$UARCH" >&2; exit 1 ;;
esac
ASSET_NAME="jerk-${OS_LOWER}-${ARCH_LOWER}"

cp "$ASSET_DIR/$ASSET_NAME" "$SERVE_ROOT/latest/download/$ASSET_NAME"
cp "$ASSET_DIR/SHA256SUMS" "$SERVE_ROOT/latest/download/SHA256SUMS"

PORT_FILE="$WORK_DIR/port"
if command -v python3 >/dev/null 2>&1; then
  python3 -c "
import http.server, socketserver, os, sys
os.chdir('$SERVE_ROOT')
h = http.server.SimpleHTTPRequestHandler
h.log_message = lambda *a: None
s = socketserver.TCPServer(('127.0.0.1', 0), h)
p = s.server_address[1]
open('$PORT_FILE', 'w').write(str(p))
s.serve_forever()
" &
  SERVER_PID=$!
elif command -v busybox >/dev/null 2>&1 && busybox --list 2>/dev/null | grep -q httpd; then
  CONF="$WORK_DIR/httpd.conf"
  printf '' > "$CONF"
  PORT=8384
  printf '%d' "$PORT" > "$PORT_FILE"
  busybox httpd -f -p "127.0.0.1:$PORT" -h "$SERVE_ROOT" -c "$CONF" &
  SERVER_PID=$!
else
  printf 'need python3 or busybox httpd\n' >&2
  exit 1
fi

cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

attempts=0
while [ ! -s "$PORT_FILE" ] && [ "$attempts" -lt 30 ]; do
  sleep 0.2
  attempts=$((attempts + 1))
done
PORT="$(cat "$PORT_FILE")"
BASE_URL="http://127.0.0.1:${PORT}"
printf 'serving on %s\n' "$BASE_URL"

printf '\n=== test: default install ===\n'
TDIR="$WORK_DIR/t1"
rc=0
output="$(SLICC_INSTALL_BASE_URL="$BASE_URL" INSTALL_DIR="$TDIR" sh "$REPO_ROOT/install.sh" 2>&1)" || rc=$?
assert_ok "$rc" "install succeeds"
assert_contains "$output" "jerk $FAKE_VERSION" "version output"
bin_rc=0
test -x "$TDIR/jerk" || bin_rc=1
assert_ok "$bin_rc" "binary is executable"

printf '\n=== test: custom --dir flag ===\n'
TDIR2="$WORK_DIR/t2/custom"
rc2=0
output2="$(SLICC_INSTALL_BASE_URL="$BASE_URL" sh "$REPO_ROOT/install.sh" --dir "$TDIR2" 2>&1)" || rc2=$?
assert_ok "$rc2" "install with --dir"
assert_contains "$output2" "jerk $FAKE_VERSION" "version with --dir"
bin2_rc=0
test -x "$TDIR2/jerk" || bin2_rc=1
assert_ok "$bin2_rc" "binary in custom dir"
assert_contains "$output2" "PATH" "PATH hint"

printf '\n=== test: idempotent re-run ===\n'
rc3=0
output3="$(SLICC_INSTALL_BASE_URL="$BASE_URL" INSTALL_DIR="$TDIR" sh "$REPO_ROOT/install.sh" 2>&1)" || rc3=$?
assert_ok "$rc3" "re-run succeeds"
assert_contains "$output3" "jerk $FAKE_VERSION" "re-run version"

printf '\n=== test: tampered asset ===\n'
TAMPER_ROOT="$WORK_DIR/tamper/latest/download"
mkdir -p "$TAMPER_ROOT"
cp "$SERVE_ROOT/latest/download/SHA256SUMS" "$TAMPER_ROOT/SHA256SUMS"
printf 'tampered' > "$TAMPER_ROOT/$ASSET_NAME"

kill "$SERVER_PID" 2>/dev/null || true
sleep 0.5

if command -v python3 >/dev/null 2>&1; then
  TPORT_FILE="$WORK_DIR/tport"
  python3 -c "
import http.server, socketserver, os
os.chdir('$WORK_DIR/tamper')
h = http.server.SimpleHTTPRequestHandler
h.log_message = lambda *a: None
s = socketserver.TCPServer(('127.0.0.1', 0), h)
open('$TPORT_FILE', 'w').write(str(s.server_address[1]))
s.serve_forever()
" &
  SERVER_PID=$!
  attempts=0
  while [ ! -s "$TPORT_FILE" ] && [ "$attempts" -lt 30 ]; do
    sleep 0.2
    attempts=$((attempts + 1))
  done
  TAMPER_PORT="$(cat "$TPORT_FILE")"
else
  TAMPER_PORT=8384
  busybox httpd -f -p "127.0.0.1:$TAMPER_PORT" -h "$WORK_DIR/tamper" -c "$CONF" &
  SERVER_PID=$!
  sleep 0.5
fi

TDIR3="$WORK_DIR/t3"
mkdir -p "$TDIR3"
tamper_rc=0
SLICC_INSTALL_BASE_URL="http://127.0.0.1:${TAMPER_PORT}" INSTALL_DIR="$TDIR3" sh "$REPO_ROOT/install.sh" 2>/dev/null || tamper_rc=$?
assert_fail "$tamper_rc" "tampered asset rejected"

printf '\n=== results: %d passed, %d failed ===\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
