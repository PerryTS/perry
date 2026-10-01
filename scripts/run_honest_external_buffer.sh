#!/usr/bin/env bash
# Build a local Node-API fixture; no package installation or network access.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
fixture="$root/.lane-tmp/honest-external-buffer"
mkdir -p "$fixture/node_modules/fixture-external-buffer"
package="$fixture/node_modules/fixture-external-buffer"
cat > "$fixture/package.json" <<'JSON'
{"name":"honest-external-buffer","private":true,"perry":{"compilePackages":["fixture-external-buffer"],"allow":{"compilePackages":["fixture-external-buffer"]},"nativeAddons":["fixture-external-buffer"]}}
JSON
cat > "$package/package.json" <<'JSON'
{"name":"fixture-external-buffer","version":"1.0.0","main":"index.js"}
JSON
printf '%s\n' 'module.exports = require("./addon.node");' > "$package/index.js"
flags=(-shared)
if [[ $(uname -s) == Darwin ]]; then flags+=(-undefined dynamic_lookup); else flags+=(-fPIC); fi
"${CC:-cc}" "${flags[@]}" "$root/test-files/fixtures/honest-external-buffer/addon.c" -o "$package/addon.node"
cp "$root/test-files/fixtures/honest-external-buffer/surface.ts" "$fixture/main.ts"
cd "$fixture"
node --experimental-strip-types main.ts > node.out
if [[ ${1:-} == --node-only ]]; then cat node.out; exit 0; fi
# Perry uses a fixed optional-runtime subdirectory for native addons. Alias
# that path to this worktree's single physical target; do not duplicate it.
if [[ ! -e "$root/target/perry-optional-runtime" ]]; then
  ln -s . "$root/target/perry-optional-runtime"
fi
if [[ ! -L "$root/target/perry-optional-runtime" ]]; then
  echo "optional-runtime must alias the single target directory" >&2
  exit 1
fi
# An unsuccessful compile must never leave an old run available as evidence.
rm -f perry.out
# Default target/ remains the only cargo target directory. Supply matching
# compiler and static wrappers with node-api-host enabled.
PERRY_NO_UPDATE_CHECK=1 PERRY_NO_TELEMETRY=1 PERRY_WORKSPACE_ROOT="$root" \
  PERRY_RUNTIME_DIR="$root/target/release" \
  "$root/target/release/perry" compile main.ts --no-cache --no-auto-optimize -o app
./app > perry.out
diff -u node.out perry.out
