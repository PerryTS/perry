#!/bin/sh
# Perry installer — downloads the latest release from GitHub
# Usage: curl -fsSL https://perryts.com/install.sh | sh

set -e

REPO="PerryTS/perry"
INSTALL_DIR="/usr/local/bin"
LIB_DIR="/usr/local/lib"

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

ARTIFACT="perry-${OS}-${ARCH}.tar.gz"

echo "Detecting platform: ${OS}/${ARCH}"

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

echo "Downloading $ARTIFACT..."

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/perry.tar.gz"

echo "Extracting..."
tar xzf "$TMPDIR/perry.tar.gz" -C "$TMPDIR"

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
