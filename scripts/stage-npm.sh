#!/usr/bin/env bash
# Stage npm packages for publishing.
#
# Reads the workspace version from Cargo.toml, extracts the per-platform
# release tarballs (as produced by .github/workflows/release-packages.yml)
# into npm/perry-<platform>/{bin,lib}, and renders each package.json.tmpl
# with the version substituted. Each cross target in TARGET_TRIPLES
# (scripts/publish/constants.mts) becomes npm/perry-target-<triple>/ from its
# perry-cross-<triple>.tar.gz bundle, and each host in EXT_TRIPLES gets
# npm/perry-ext-<triple>/ from its release archive's libperry_ext_*.a
# (scripts/publish/stage-target.mts).
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
# A cross bundle is perry-cross-<triple>.tar.gz, either flat in
# <artifact-dir> or inside its perry-cross-<triple>/ artifact subdir.
#
# Env:
#   SKIP_MISSING=1    Don't fail if an expected platform artifact or cross
#                     bundle is absent. Local testing only: CI never sets it,
#                     so a release cannot ship the wrapper without a target.
#   KEEP_EXISTING=1   Don't wipe npm/perry-*/bin|lib before staging.
#   PERRY_SOURCE_REVISION
#                     The release's full commit sha. Written into each
#                     target.json; required for a bundle whose manifest has
#                     no source_revision, and checked against one that has.

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
# Platform mapping: <npm-package-dir-suffix>:<release-artifact-name>:<ui-lib-basename>:<rust-triple>
# Release artifact names match the `artifact:` field in release-packages.yml.
# The triple is the one the perry binary in the archive is built for; it names
# the host's ext package (@perryts/perry-ext-<triple>).
# On Windows the ui lib has a .lib extension and the binary is perry.exe.
# -----------------------------------------------------------------------------
PLATFORMS=(
  "darwin-arm64:perry-macos-aarch64:libperry_ui_macos.a:aarch64-apple-darwin"
  "darwin-x64:perry-macos-x86_64:libperry_ui_macos.a:x86_64-apple-darwin"
  "linux-x64:perry-linux-x86_64:libperry_ui_gtk4.a:x86_64-unknown-linux-gnu"
  "linux-arm64:perry-linux-aarch64:libperry_ui_gtk4.a:aarch64-unknown-linux-gnu"
  "linux-x64-musl:perry-linux-x86_64-musl:libperry_ui_gtk4.a:x86_64-unknown-linux-musl"
  "linux-arm64-musl:perry-linux-aarch64-musl:libperry_ui_gtk4.a:aarch64-unknown-linux-musl"
  "win32-x64:perry-windows-x86_64:perry_ui_windows.lib:x86_64-pc-windows-msvc"
  "win32-arm64:perry-windows-aarch64:perry_ui_windows.lib:aarch64-pc-windows-msvc"
)

# Unix libs shared across platforms (runtime + stdlib). The UI lib is
# handled per-platform above. Windows equivalents are baked into the case
# block further down.
UNIX_CORE_LIBS=(libperry_runtime.a libperry_stdlib.a)
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

# Strip embedded LLVM bitcode (.llvmbc/.llvmcmd) from an archive's members
# before it is compressed. The UI and wasm-host archives bundle the Rust
# standard library from rustup's precompiled rlibs, whose objects carry their
# bitcode (about 10 MB raw, 2.5-3 MB compressed per archive); a native link
# never reads it, and no cargo flag removes it. llvm-objcopy rejects a whole
# Windows .lib (import members), so every platform takes the same path: member
# by member, original order and names, symbol table rebuilt, no other byte of
# any object touched (scripts/strip-archive-members.py). Extra args pass
# through (--strip-debug). Darwin archives (BSD layout) are not handled and
# stay as built. Args: [--strip-debug] archive...
strip_archive_bitcode() {
  python3 "$REPO_ROOT/scripts/strip-archive-members.py" "$@"
}

# GC effects sidecars: `<archive>.gc-effects` is staged beside every runtime
# archive (and, once the archive is compressed, beside its .a.zst). perry
# re-hashes the decompressed archive against it at compile time. A runtime
# archive without its sidecar is an error: perry would fall back to the
# all-unsafe table. Args: $1 = source dir, $2 = dest dir, rest = archive names
# that were staged (non-runtime archives have no sidecar and are skipped).
stage_sidecars() {
  local src="$1" dest="$2" lib
  shift 2
  for lib in "$@"; do
    case "$lib" in
      *perry_runtime*) ;;
      *) continue ;;
    esac
    [ -f "$dest/$lib" ] || continue
    if [ ! -f "$src/$lib.gc-effects" ]; then
      echo "  error: $src has no $lib.gc-effects sidecar (scripts/gc_call_effects/sidecars.sh)" >&2
      exit 1
    fi
    cp "$src/$lib.gc-effects" "$dest/$lib.gc-effects"
  done
}

# zstd every archive in $1 (sidecars stay raw: perry reads them beside the
# candidate path). Set PERRY_NPM_NO_COMPRESS=1 for local staging where
# uncompressed libs are handy; perry finds either form.
compress_archives() {
  local dir="$1" f
  [ "${PERRY_NPM_NO_COMPRESS:-0}" != "1" ] && [ -d "$dir" ] || return 0
  if ! command -v zstd >/dev/null 2>&1; then
    echo "  error: zstd not found but archive compression is required" >&2
    echo "         install zstd or set PERRY_NPM_NO_COMPRESS=1" >&2
    exit 1
  fi
  for f in "$dir"/*; do
    [ -f "$f" ] || continue
    case "$f" in
      *.zst | *.gc-effects) continue ;;
    esac
    zstd -19 -T0 -q -f --rm "$f"
    echo "  compressed $(basename "$f") -> $(basename "$f").zst"
  done
}

# Native-extension archives (`libperry_ext_*.a`): `perry compile` links one per
# well-known module a program imports (http, ws, net, zlib, sharp, ...). They
# total ~140-166 MB compressed per host, and the host packages are already
# near the 180 MB packed budget, so they ship in a package of their own,
# `@perryts/perry-ext-<triple>` at npm/perry-ext-<triple>, for every triple in
# EXT_TRIPLES (constants.mts). perry finds it beside itself or fetches it from
# the GitHub release on the first compile that needs it.
# Args: $1 = host triple, $2 = source dir of the host's release archive.
stage_ext_package() {
  local triple="$1" src="$2"
  local ext_dir="$NPM_DIR/perry-ext-$triple"
  echo "[stage-npm] ext: $triple  <-  $(basename "$src")"
  node "$REPO_ROOT/scripts/publish/stage-target.mts" --ext "$src" "$ext_dir" "$triple" "$VERSION"
  compress_archives "$ext_dir/lib"
}

# Read one list from scripts/publish/constants.mts (one triple per line).
# Args: $1 = --print-targets | --print-ext. Prints the triples.
read_triples() {
  local out
  out="$(node "$REPO_ROOT/scripts/publish/constants.mts" "$1")"
  if [ -z "$out" ]; then
    echo "error: scripts/publish/constants.mts $1 printed no triples" >&2
    exit 1
  fi
  printf '%s\n' "$out"
}

in_list() {
  local needle="$1" item
  shift
  for item in "$@"; do
    [ "$item" = "$needle" ] && return 0
  done
  return 1
}

EXT_TRIPLES=()
while IFS= read -r triple; do EXT_TRIPLES+=("$triple"); done < <(read_triples --print-ext)
staged_ext=0

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
  IFS=: read -r pkg_suffix artifact ui_lib host_triple <<< "$entry"
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
    # #7985: perry.exe imports the official LLVM archive's LLVM-C.dll to avoid
    # mixing its /MT + rpmalloc static objects with Rust's /MD runtime. The DLL
    # must stay beside the executable: Windows searches that directory first,
    # and npm installs do not inherit CI's C:\llvm\bin PATH entry.
    if [ ! -f "$src_dir/LLVM-C.dll" ]; then
      echo "  error: $artifact is missing LLVM-C.dll required by perry.exe (#7985)" >&2
      exit 1
    fi
    cp "$src_dir/perry.exe" "$pkg_dir/bin/perry.exe"
    cp "$src_dir/LLVM-C.dll" "$pkg_dir/bin/LLVM-C.dll"
    for lib in "${WIN_CORE_LIBS[@]}" perry_wasm_host.lib "$ui_lib"; do
      [ -f "$src_dir/$lib" ] && cp "$src_dir/$lib" "$pkg_dir/lib/"
    done
    for lib in perry_wasm_host.lib "$ui_lib"; do
      [ -f "$pkg_dir/lib/$lib" ] && strip_archive_bitcode "$pkg_dir/lib/$lib"
    done
    # The vendored OpenSSL (openssl-src) is compiled by OpenSSL's own MSVC
    # Configure target, which hardcodes `/Zi /Fdossl_static.pdb` for every
    # library object and has no switch to turn it off; the CodeView records
    # (about 37 MB raw) point at a PDB that is not shipped. Drop them with the
    # bitcode.
    [ -f "$pkg_dir/lib/perry_stdlib.lib" ] && strip_archive_bitcode --strip-debug "$pkg_dir/lib/perry_stdlib.lib"
    stage_sidecars "$src_dir" "$pkg_dir/lib" "${WIN_CORE_LIBS[@]}"
    # No android cross-libs here: with them (#4823) the Windows packages
    # packed to ~230 MB and npm rejected the upload (413 Payload Too Large).
    # They ship once, in @perryts/perry-target-aarch64-linux-android, for
    # every host (staged below).
  else
    cp "$src_dir/perry" "$pkg_dir/bin/perry"
    chmod +x "$pkg_dir/bin/perry"
    for lib in "${UNIX_CORE_LIBS[@]}" libperry_wasm_host.a "$ui_lib"; do
      [ -f "$src_dir/$lib" ] && cp "$src_dir/$lib" "$pkg_dir/lib/"
    done
    case "$pkg_suffix" in
      darwin-*) ;; # BSD-layout archives: see strip_archive_bitcode
      *)
        for lib in libperry_wasm_host.a "$ui_lib"; do
          [ -f "$pkg_dir/lib/$lib" ] && strip_archive_bitcode "$pkg_dir/lib/$lib"
        done
        ;;
    esac
    stage_sidecars "$src_dir" "$pkg_dir/lib" "${UNIX_CORE_LIBS[@]}"
  fi
  # The host's ext archives stay out of its package (see stage_ext_package).
  if in_list "$host_triple" "${EXT_TRIPLES[@]}"; then
    stage_ext_package "$host_triple" "$src_dir"
    staged_ext=$((staged_ext + 1))
  fi

  # Compress the static archives so the published npm tarball stays under
  # npm's registry upload limit (the raw archives total ~750 MB per platform;
  # npm rejects the upload with HTTP 413). The perry binary decompresses them
  # transparently on first use into a per-user cache (see compressed_libs.rs).
  # The binary in bin/ is left raw — it is exec'd directly.
  compress_archives "$pkg_dir/lib"

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

# -----------------------------------------------------------------------------
# Stage each target package: @perryts/perry-target-<triple>, one per cross
# triple, host-independent (no os/cpu). The list lives in constants.mts only.
# -----------------------------------------------------------------------------
TARGET_TRIPLES=()
while IFS= read -r triple; do TARGET_TRIPLES+=("$triple"); done < <(read_triples --print-targets)

staged_targets=0
for triple in "${TARGET_TRIPLES[@]}"; do
  bundle_name="perry-cross-$triple"
  pkg_dir="$NPM_DIR/perry-target-$triple"
  echo "[stage-npm] target: $triple  <-  $bundle_name.tar.gz"
  bundle=""
  for candidate in "$ARTIFACT_DIR/$bundle_name.tar.gz" "$ARTIFACT_DIR/$bundle_name/$bundle_name.tar.gz"; do
    if [ -f "$candidate" ]; then bundle="$candidate"; break; fi
  done
  if [ -z "$bundle" ]; then
    if [ "${SKIP_MISSING:-0}" = "1" ]; then
      echo "  (skip: no bundle found)"
      continue
    fi
    # Never ship the wrapper without its targets: a missing bundle (a red
    # build-cross or build-wasi leg) fails the npm leg before any publish.
    echo "  error: no $bundle_name.tar.gz in $ARTIFACT_DIR (set SKIP_MISSING=1 to ignore)" >&2
    exit 1
  fi
  workdir="$ARTIFACT_DIR/.extracted/$bundle_name"
  rm -rf "$workdir"
  mkdir -p "$workdir"
  tar xzf "$bundle" -C "$workdir"
  node "$REPO_ROOT/scripts/publish/stage-target.mts" "$workdir" "$pkg_dir" "$triple" "$VERSION"
  compress_archives "$pkg_dir/lib"
  staged_targets=$((staged_targets + 1))
done

# Every listed ext triple must have a PLATFORMS row (and so a release archive).
if [ "$staged_ext" -ne "${#EXT_TRIPLES[@]}" ] && [ "${SKIP_MISSING:-0}" != "1" ]; then
  echo "error: staged $staged_ext of ${#EXT_TRIPLES[@]} ext packages (${EXT_TRIPLES[*]}); each needs a PLATFORMS row" >&2
  exit 1
fi

echo "[stage-npm] done. Version $VERSION staged across $(ls -d "$NPM_DIR"/perry-*/bin 2>/dev/null | wc -l | tr -d ' ') platform packages, $staged_ext ext packages and $staged_targets target packages."
