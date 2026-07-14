#!/bin/sh
# Perry installer — downloads the latest release from GitHub
# Usage: curl -fsSL https://perryts.com/install.sh | sh

set -eu

REPO="PerryTS/perry"
INSTALL_DIR="/usr/local/bin"
LIB_DIR="/usr/local/lib"

# Only show progress bars on interactive terminals
if [ -t 2 ]; then
  CURL_PROGRESS="--progress-bar"
  PV=""
  if command -v pv >/dev/null 2>&1; then
    PV="pv"
  fi
else
  CURL_PROGRESS="-fsS"
  PV=""
fi

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  darwin) OS="macos" ;;
  linux)  OS="linux" ;;
  *)
    echo "Error: Unsupported OS: $OS"
    echo "See https://github.com/$REPO for manual install instructions."
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)
    echo "Error: Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

# Pick the glibc or the fully-static (musl) Linux build.
#
# The Linux glibc tarballs are built by .github/workflows/release-packages.yml
# on ubuntu-24.04 / ubuntu-24.04-arm, whose glibc is 2.39. Their ELF carries
# GLIBC_2.39 symbol-version references, so the dynamic loader refuses to start
# them on anything older — Ubuntu 22.04 (2.35), Debian 12 (2.36), RHEL 9 and
# Amazon Linux 2023 (2.34) all die with "GLIBC_2.39 not found" before Perry runs
# (#6298). The musl tarball is a fully-static binary with no interpreter and no
# libc dependency at all, so it runs on every one of them.
#
# KEEP IN SYNC with the builder image and with GLIBC_BUILD_FLOOR in
# npm/perry/bin/detect.cjs.
GLIBC_FLOOR_MAJOR=2
GLIBC_FLOOR_MINOR=39

LIBC="glibc"
if [ "$OS" = "linux" ]; then
  if [ -f /etc/alpine-release ] || (ldd --version 2>&1 | head -1 | grep -qi musl); then
    LIBC="musl"
    echo "Detected musl libc — using the fully-static build."
    echo "(Note: the static build cannot build perry/ui GTK4 apps.)"
  else
    # getconf: "glibc 2.35". ldd: "ldd (Ubuntu GLIBC 2.35-0ubuntu3) 2.35".
    GLIBC_VER=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')
    if [ -z "$GLIBC_VER" ]; then
      GLIBC_VER=$(ldd --version 2>/dev/null | head -1 | awk '{print $NF}')
    fi
    GLIBC_MAJOR=$(printf '%s' "$GLIBC_VER" | cut -d. -f1 | tr -cd '0-9')
    GLIBC_MINOR=$(printf '%s' "$GLIBC_VER" | cut -d. -f2 | tr -cd '0-9')
    # Unparseable (empty / non-numeric) → keep the glibc build. Guessing "too
    # old" for every host we can't read would push everyone onto the static
    # build, which cannot build perry/ui (GTK4) apps.
    if [ -n "$GLIBC_MAJOR" ] && [ -n "$GLIBC_MINOR" ]; then
      if [ "$GLIBC_MAJOR" -lt "$GLIBC_FLOOR_MAJOR" ] ||
        { [ "$GLIBC_MAJOR" -eq "$GLIBC_FLOOR_MAJOR" ] && [ "$GLIBC_MINOR" -lt "$GLIBC_FLOOR_MINOR" ]; }; then
        LIBC="musl"
        echo "Detected glibc ${GLIBC_VER} — older than the ${GLIBC_FLOOR_MAJOR}.${GLIBC_FLOOR_MINOR} the"
        echo "prebuilt glibc binary needs. Using the fully-static Linux build instead."
        echo "(Same compiler; it cannot build perry/ui GTK4 apps. See issue #6298.)"
      fi
    fi
  fi
fi

if [ "$LIBC" = "musl" ]; then
  ARTIFACT="perry-${OS}-${ARCH}-musl.tar.gz"
else
  ARTIFACT="perry-${OS}-${ARCH}.tar.gz"
fi

echo "Detecting platform: ${OS}/${ARCH} (${LIBC})"

# Find the most recent release that actually has our platform asset.
#
# We can't blindly use `releases/latest`: when the release-packages workflow
# fails its test gate (or hasn't finished yet), the tag still publishes but
# arrives with zero assets. Pre-fix the script downloaded `releases/latest`
# unconditionally and 404'd on every Linux install when the most recent
# tagged release happened to have no assets. Instead, list recent tags and
# probe each tarball URL until one returns 200/302.
echo "Locating most recent release with $ARTIFACT..."

# Pull the tag_name of the 30 most recent releases. Stays POSIX (sed + grep)
# rather than relying on jq/gawk so the script runs on any /bin/sh.
TAGS=$(
  curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=30" \
    | grep '"tag_name":' \
    | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
)

LATEST=""
for tag in $TAGS; do
  url="https://github.com/$REPO/releases/download/$tag/$ARTIFACT"
  # -I = HEAD, -L = follow redirects, -o /dev/null + -w "%{http_code}"
  # gives just the final status. 200 (direct) and 302 (the GitHub release
  # download → S3 redirect, when followed lands on 200) both mean asset
  # present. We accept 200 only since -L was passed.
  # `curl -L -I` issues HEAD against each redirect hop; -w "%{http_code}"
  # then prints every hop's status concatenated (e.g. "404000" if the first
  # response is 404 and curl continues). The final hop's status is always
  # the last 3 chars; "200" there means "asset exists and downloads cleanly".
  status=$(curl -fsSLI -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
  final="${status#"${status%???}"}"  # last 3 chars, POSIX-portable
  if [ "$final" = "200" ]; then
    LATEST="$tag"
    break
  fi
done

if [ -z "$LATEST" ]; then
  echo "Error: No recent release has $ARTIFACT attached."
  echo "       Releases list: https://github.com/$REPO/releases"
  echo "       Try: npm install -g @perryts/perry  (always works)"
  exit 1
fi

echo "Using version: $LATEST"

URL="https://github.com/$REPO/releases/download/$LATEST/$ARTIFACT"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading $ARTIFACT..."
START=$(date +%s 2>/dev/null || echo 0)
curl -fL $CURL_PROGRESS -o "$TMPDIR/perry.tar.gz" "$URL"
END=$(date +%s 2>/dev/null || echo 0)

if [ "$END" -gt "$START" ] && [ "$START" -gt 0 ]; then
  echo "  Done in $((END - START))s ($(ls -lh "$TMPDIR/perry.tar.gz" | awk '{print $5}'))"
fi

echo "Extracting..."
if [ -n "$PV" ]; then
  pv "$TMPDIR/perry.tar.gz" | tar xz -C "$TMPDIR"
else
  tar xzf "$TMPDIR/perry.tar.gz" -C "$TMPDIR"
fi

# Install binary
if [ -w "$INSTALL_DIR" ]; then
  cp "$TMPDIR/perry" "$INSTALL_DIR/perry"
  chmod 755 "$INSTALL_DIR/perry"
  # Install libraries alongside binary
  for lib in "$TMPDIR"/libperry_*.a; do
    [ -f "$lib" ] && cp "$lib" "$LIB_DIR/"
  done
else
  echo "Installing to $INSTALL_DIR (requires sudo)..."
  sudo cp "$TMPDIR/perry" "$INSTALL_DIR/perry"
  sudo chmod 755 "$INSTALL_DIR/perry"
  for lib in "$TMPDIR"/libperry_*.a; do
    [ -f "$lib" ] && sudo cp "$lib" "$LIB_DIR/"
  done
fi

echo ""
echo "Perry $LATEST installed successfully!"
echo ""
echo "Quick start:"
echo "  echo 'console.log(\"hello\")' > hello.ts"
echo "  perry hello.ts -o hello && ./hello"
echo ""
echo "Run 'perry doctor' to verify your setup."
