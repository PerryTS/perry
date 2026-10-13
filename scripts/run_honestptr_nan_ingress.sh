#!/usr/bin/env bash
# A local Node-API payload-NaN fixture; no package installation.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
fixture="$root/.lane-tmp/honestptr-nan"
package="$fixture/node_modules/fixture-honestptr-nan"
mkdir -p "$package"
cat > "$fixture/package.json" <<'JSON'
{"name":"honestptr-nan","private":true,"type":"module","perry":{"compilePackages":["fixture-honestptr-nan"],"allow":{"compilePackages":["fixture-honestptr-nan"]},"nativeAddons":["fixture-honestptr-nan"]}}
JSON
cat > "$package/package.json" <<'JSON'
{"name":"fixture-honestptr-nan","version":"1.0.0","main":"index.js"}
JSON
printf '%s\n' 'module.exports = require("./addon.node");' > "$package/index.js"
flags=(-shared)
if [[ $(uname -s) == Darwin ]]; then flags+=(-undefined dynamic_lookup); else flags+=(-fPIC); fi
"${CC:-cc}" "${flags[@]}" "$root/test-files/fixtures/honestptr-nan/addon.c" -o "$package/addon.node"
cp "$root/test-files/fixtures/honestptr-nan/surface.ts" "$fixture/main.ts"
cd "$fixture"
node --expose-gc --experimental-strip-types main.ts > node.out
if [[ ${1:-} == --node-only ]]; then cat node.out; exit 0; fi
: "${CARGO_TARGET_DIR:?set the lane target directory}"
: "${PERRY_RUNTIME_DIR:?set the matching static archive directory}"
PERRY_NO_UPDATE_CHECK=1 PERRY_NO_TELEMETRY=1 \
  "$CARGO_TARGET_DIR/release/perry" compile main.ts --no-cache --no-auto-optimize -o app
./app > perry.out
diff -u node.out perry.out
