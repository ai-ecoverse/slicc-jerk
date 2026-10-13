#!/bin/sh
set -eu

usage() {
  printf 'Usage: install.sh [--version VERSION] [--dir INSTALL_DIR] [--help]\n'
  printf '\n'
  printf 'Environment variables:\n'
  printf '  VERSION              version to install (default: latest)\n'
  printf '  INSTALL_DIR          target directory (default: %s/.local/bin)\n' "\$HOME"
  printf '  SLICC_INSTALL_BASE_URL  override download base URL\n'
  exit 0
}

VERSION="${VERSION:-}"
INSTALL_DIR="${INSTALL_DIR:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir)     INSTALL_DIR="$2"; shift 2 ;;
    --help)    usage ;;
    *)         printf 'unknown option: %s\n' "$1" >&2; exit 1 ;;
  esac
done

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

OS="$(uname -s)"
case "$OS" in
  Darwin) OS=darwin ;;
  Linux)  OS=linux ;;
  *)
    printf 'Unsupported OS: %s\n' "$OS" >&2
    printf 'Install via npm instead: npm i -g @ai-ecoverse/slicc-jerk\n' >&2
    exit 1
    ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)   ARCH=amd64 ;;
  arm64|aarch64)   ARCH=arm64 ;;
  *)
    printf 'Unsupported architecture: %s\n' "$ARCH" >&2
    exit 1
    ;;
esac

fetch_to() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$1"
  else
    printf 'neither curl nor wget found\n' >&2
    exit 1
  fi
}

checksum_verify() {
  _file="$1"
  _expected="$2"
  if command -v sha256sum >/dev/null 2>&1; then
    _actual="$(sha256sum "$_file" | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    _actual="$(shasum -a 256 "$_file" | cut -d' ' -f1)"
  else
    printf 'neither sha256sum nor shasum found\n' >&2
    exit 1
  fi
  if [ "$_actual" != "$_expected" ]; then
    printf 'checksum mismatch for %s\n' "$_file" >&2
    printf '  expected: %s\n' "$_expected" >&2
    printf '  got:      %s\n' "$_actual" >&2
    exit 1
  fi
}

BASE_URL="${SLICC_INSTALL_BASE_URL:-https://github.com/ai-ecoverse/slicc-jerk/releases}"

if [ -z "$VERSION" ]; then
  DOWNLOAD_BASE="$BASE_URL/latest/download"
else
  DOWNLOAD_BASE="$BASE_URL/download/v$VERSION"
fi

ASSET="jerk-${OS}-${ARCH}"

TMPDIR_WORK="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_WORK"' EXIT

fetch_to "$DOWNLOAD_BASE/SHA256SUMS" "$TMPDIR_WORK/SHA256SUMS"
fetch_to "$DOWNLOAD_BASE/$ASSET" "$TMPDIR_WORK/$ASSET"

EXPECTED="$(grep "  ${ASSET}$" "$TMPDIR_WORK/SHA256SUMS" | cut -d' ' -f1 || true)"
if [ -z "$EXPECTED" ]; then
  printf 'no checksum for %s in SHA256SUMS\n' "$ASSET" >&2
  exit 1
fi

checksum_verify "$TMPDIR_WORK/$ASSET" "$EXPECTED"

mkdir -p "$INSTALL_DIR"

TMPBIN="$(mktemp "$INSTALL_DIR/jerk.XXXXXX")"
cp "$TMPDIR_WORK/$ASSET" "$TMPBIN"
chmod 755 "$TMPBIN"
mv "$TMPBIN" "$INSTALL_DIR/jerk"

printf '%s\n' "$("$INSTALL_DIR/jerk" --version)"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    printf 'Add %s to your PATH:\n' "$INSTALL_DIR"
    _dollar='$'
    printf '  export PATH="%s:%sPATH"\n' "$INSTALL_DIR" "$_dollar"
    ;;
esac
