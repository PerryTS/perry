#!/usr/bin/env bash
# Stage npm packages for publishing.
#
# Reads the workspace version from Cargo.toml, extracts the per-platform
# release tarballs (as produced by .github/workflows/release-packages.yml)
# into npm/perry-<platform>/{bin,lib}, and renders each package.json.tmpl
# with the version substituted.
#
# Usage:
#   scripts/stage-npm.sh <artifact-dir>
#
# <artifact-dir> is expected to contain the release tarballs. Two layouts
# are supported:
#   (a) Flat: perry-macos-aarch64.tar.gz, perry-linux-x86_64.tar.gz, ...
#   (b) actions/download-artifact layout: one subdir per artifact name,
#       each containing the already-extracted staging/ contents
#       (perry binary + lib*.a).
#
# Env:
#   SKIP_MISSING=1    Don't fail if an expected platform artifact is absent.
#                     Useful for iterative local testing.
#   KEEP_EXISTING=1   Don't wipe npm/perry-*/bin|lib before staging.

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <artifact-dir>" >&2
  exit 2
fi

ARTIFACT_DIR="$1"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NPM_DIR="$REPO_ROOT/npm"
LICENSE_SRC="$REPO_ROOT/LICENSE"

if [ ! -d "$ARTIFACT_DIR" ]; then
  echo "error: artifact dir not found: $ARTIFACT_DIR" >&2
  exit 1
fi

# -----------------------------------------------------------------------------
# Read workspace version from Cargo.toml ([workspace.package] → version = "x.y.z")
# -----------------------------------------------------------------------------
VERSION="$(awk '
  /^\[workspace\.package\]/ { in_section = 1; next }
  /^\[/                     { in_section = 0 }
  in_section && /^version[[:space:]]*=/ {
    gsub(/"/, "", $3); print $3; exit
  }
' "$REPO_ROOT/Cargo.toml")"

if [ -z "$VERSION" ]; then
  echo "error: could not parse version from Cargo.toml" >&2
  exit 1
fi
echo "[stage-npm] version = $VERSION"

# -----------------------------------------------------------------------------
# Platform mapping: <npm-package-dir-suffix>:<release-artifact-name>:<ui-lib-basename>
# Release artifact names match the `artifact:` field in release-packages.yml.
# On Windows the ui lib has a .lib extension and the binary is perry.exe.
# -----------------------------------------------------------------------------
PLATFORMS=(
  "darwin-arm64:perry-macos-aarch64:libperry_ui_macos.a"
  "darwin-x64:perry-macos-x86_64:libperry_ui_macos.a"
  "linux-x64:perry-linux-x86_64:libperry_ui_gtk4.a"
  "linux-arm64:perry-linux-aarch64:libperry_ui_gtk4.a"
  "linux-x64-musl:perry-linux-x86_64-musl:libperry_ui_gtk4.a"
  "linux-arm64-musl:perry-linux-aarch64-musl:libperry_ui_gtk4.a"
  "win32-x64:perry-windows-x86_64:perry_ui_windows.lib"
)

# Unix libs shared across platforms (runtime + stdlib). The UI lib is
# handled per-platform above. Windows equivalents are baked into the case
# block further down.
UNIX_CORE_LIBS=(libperry_runtime.a libperry_runtime_abort.a libperry_stdlib.a)
WIN_CORE_LIBS=(perry_runtime.lib perry_stdlib.lib)

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------
render_template() {
  # $1 = template path, $2 = output path
  sed "s/__VERSION__/$VERSION/g" "$1" > "$2"
}

# Returns path to an extracted staging dir for a given artifact name, or
# empty string if not found. Handles both flat tarballs and the
# download-artifact subdir layout.
resolve_artifact() {
  local artifact="$1"                 # e.g. perry-macos-aarch64
  local kind="$2"                     # "unix" (tar.gz) or "win" (zip)
  local workdir="$ARTIFACT_DIR/.extracted/$artifact"

  # (b) download-artifact subdir layout: binary/libs already extracted.
  if [ -d "$ARTIFACT_DIR/$artifact" ]; then
    # Trust what's in there; return it directly.
    echo "$ARTIFACT_DIR/$artifact"
    return 0
  fi

  # (a) flat archive layout: extract once, cache in .extracted/.
  local archive
  if [ "$kind" = "win" ]; then
    archive="$ARTIFACT_DIR/${artifact}.zip"
  else
    archive="$ARTIFACT_DIR/${artifact}.tar.gz"
  fi

  if [ ! -f "$archive" ]; then
    echo ""
    return 0
  fi

  if [ ! -d "$workdir" ]; then
    mkdir -p "$workdir"
    if [ "$kind" = "win" ]; then
      unzip -q "$archive" -d "$workdir"
    else
      tar xzf "$archive" -C "$workdir"
    fi
  fi
  echo "$workdir"
}

# #4823: Stage Android cross-compile libs into a platform package.
#
# `perry --target android` links three static archives (runtime, stdlib, UI).
# We mirror the layout perry's library_search.rs probes —
# `<bin-dir>/aarch64-linux-android/release/<lib>` and its `.a.zst` sibling —
# under the package's `bin/` dir (already covered by the npm `files`
# allowlist, so no package.json change is needed).
#
# Each lib is sourced from the host package's own build artifact first (the
# Windows release leg cross-builds all three — see release-packages.yml). The
# UI lib is best-effort on that leg, so it falls back to the standalone
# `perry-cross-aarch64-linux-android` bundle the Android cross-bundle job
# always produces. This both enables npm-installed android builds (previously
# the npm packages shipped no android libs at all) and backfills the UI lib
# when the Windows-side cross-build was skipped.
#
# Args: $1 = package dir, $2 = host artifact src dir (may lack the subdir).
stage_android_cross_libs() {
  local pkg_dir="$1" host_src="$2"
  local triple="aarch64-linux-android"
  local host_sub="$host_src/$triple/release"
  local dest="$pkg_dir/bin/$triple/release"

  # Extract the standalone cross bundle once (fallback source for any lib the
  # host artifact is missing). download-artifact nests by artifact name; also
  # tolerate a flat drop for local runs.
  local cross_dir="" cb="$ARTIFACT_DIR/perry-cross-$triple/perry-cross-$triple.tar.gz"
  [ -f "$cb" ] || cb="$ARTIFACT_DIR/perry-cross-$triple.tar.gz"
  if [ -f "$cb" ]; then
    cross_dir="$ARTIFACT_DIR/.extracted/perry-cross-$triple"
    if [ ! -d "$cross_dir" ]; then
      mkdir -p "$cross_dir"
      tar xzf "$cb" -C "$cross_dir"
    fi
  fi

  mkdir -p "$dest"
  local staged=0 lib src
  for lib in libperry_runtime.a libperry_stdlib.a libperry_ui_android.a; do
    src=""
    if [ -f "$host_sub/$lib" ]; then
      src="$host_sub/$lib"
    elif [ -n "$cross_dir" ] && [ -f "$cross_dir/$lib" ]; then
      src="$cross_dir/$lib"
    fi
    if [ -z "$src" ]; then
      echo "  (android: $lib unavailable — skipping)"
      continue
    fi
    cp "$src" "$dest/$lib"
    staged=$((staged + 1))
  done

  if [ "$staged" -eq 0 ]; then
    rm -rf "$pkg_dir/bin/$triple"
    echo "  (android: no cross-libs staged)"
    return 0
  fi

  # Compress to match the rest of the package (raw .a's blow npm's upload
  # limit; the binary decompresses the .a.zst transparently — see
  # compressed_libs.rs). The existing lib/ compression loop only walks lib/,
  # so these bin/-subdir archives must be compressed here.
  if [ "${PERRY_NPM_NO_COMPRESS:-0}" != "1" ]; then
    if ! command -v zstd >/dev/null 2>&1; then
      echo "  error: zstd not found but archive compression is required" >&2
      exit 1
    fi
    for f in "$dest"/*.a; do
      [ -f "$f" ] || continue
      zstd -19 -T0 -q -f --rm "$f"
      echo "  compressed android/$(basename "$f") -> $(basename "$f").zst"
    done
  fi
  echo "  android: staged $staged cross-lib(s) under bin/$triple/release/"
}

# -----------------------------------------------------------------------------
# Stage wrapper package
# -----------------------------------------------------------------------------
echo "[stage-npm] wrapper: npm/perry"
render_template "$NPM_DIR/perry/package.json.tmpl" "$NPM_DIR/perry/package.json"
if [ -f "$LICENSE_SRC" ]; then
  cp "$LICENSE_SRC" "$NPM_DIR/perry/LICENSE"
fi
chmod +x "$NPM_DIR/perry/bin/perry.js"

# -----------------------------------------------------------------------------
# Stage each platform package
# -----------------------------------------------------------------------------
for entry in "${PLATFORMS[@]}"; do
  IFS=: read -r pkg_suffix artifact ui_lib <<< "$entry"
  pkg_dir="$NPM_DIR/perry-$pkg_suffix"
  echo "[stage-npm] platform: $pkg_suffix  <-  $artifact"

  case "$pkg_suffix" in
    win32-*) kind="win"  ;;
    *)       kind="unix" ;;
  esac

  src_dir="$(resolve_artifact "$artifact" "$kind")"
  if [ -z "$src_dir" ]; then
    if [ "${SKIP_MISSING:-0}" = "1" ]; then
      echo "  (skip: no artifact found)"
      continue
    fi
    echo "  error: no artifact found for $artifact (set SKIP_MISSING=1 to ignore)" >&2
    exit 1
  fi

  if [ "${KEEP_EXISTING:-0}" != "1" ]; then
    rm -rf "$pkg_dir/bin" "$pkg_dir/lib"
  fi
  mkdir -p "$pkg_dir/bin" "$pkg_dir/lib"

  if [ "$kind" = "win" ]; then
    cp "$src_dir/perry.exe" "$pkg_dir/bin/perry.exe"
    for lib in "${WIN_CORE_LIBS[@]}" "$ui_lib"; do
      [ -f "$src_dir/$lib" ] && cp "$src_dir/$lib" "$pkg_dir/lib/"
    done
    # #4823: the Windows package also carries the android cross-libs so
    # `perry --target android` works from an npm install (parity with the zip).
    stage_android_cross_libs "$pkg_dir" "$src_dir"
  else
    cp "$src_dir/perry" "$pkg_dir/bin/perry"
    chmod +x "$pkg_dir/bin/perry"
    for lib in "${UNIX_CORE_LIBS[@]}" "$ui_lib"; do
      [ -f "$src_dir/$lib" ] && cp "$src_dir/$lib" "$pkg_dir/lib/"
    done
  fi

  # Compress the static archives so the published npm tarball stays under
  # npm's registry upload limit (the raw archives total ~750 MB per platform;
  # npm rejects the upload with HTTP 413). The perry binary decompresses them
  # transparently on first use into a per-user cache (see compressed_libs.rs).
  # The binary in bin/ is left raw — it is exec'd directly. Set
  # PERRY_NPM_NO_COMPRESS=1 for local staging where uncompressed libs are handy.
  if [ "${PERRY_NPM_NO_COMPRESS:-0}" != "1" ] && [ -d "$pkg_dir/lib" ]; then
    if ! command -v zstd >/dev/null 2>&1; then
      echo "  error: zstd not found but archive compression is required" >&2
      echo "         install zstd or set PERRY_NPM_NO_COMPRESS=1" >&2
      exit 1
    fi
    for f in "$pkg_dir"/lib/*; do
      [ -f "$f" ] || continue
      case "$f" in
        *.zst) continue ;;
      esac
      zstd -19 -T0 -q -f --rm "$f"
      echo "  compressed $(basename "$f") -> $(basename "$f").zst"
    done
  fi

  render_template "$pkg_dir/package.json.tmpl" "$pkg_dir/package.json"
  if [ -f "$LICENSE_SRC" ]; then
    cp "$LICENSE_SRC" "$pkg_dir/LICENSE"
  fi
  # Minimal per-platform README (visible on npmjs.com)
  cat > "$pkg_dir/README.md" <<EOF
# @perryts/perry-$pkg_suffix

Prebuilt Perry compiler binary + static libraries for \`$pkg_suffix\`.

This package is an internal artifact of \`@perryts/perry\`. Install that instead:

\`\`\`bash
npm install -g @perryts/perry
\`\`\`
EOF
done

echo "[stage-npm] done. Version $VERSION staged across $(ls -d "$NPM_DIR"/perry-*/ 2>/dev/null | wc -l | tr -d ' ') platform packages."
