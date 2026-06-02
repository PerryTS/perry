#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [ ! -x "$PERRY" ]; then
  PERRY="$REPO_ROOT/target/debug/perry"
fi
if [ ! -x "$PERRY" ]; then
  echo "Perry binary not found; build target/release/perry or target/debug/perry first" >&2
  exit 1
fi

if [ -n "${NODE_BIN:-}" ]; then
  NODE="$NODE_BIN"
elif [ -x /tmp/perry-node25-bin/node ]; then
  NODE=/tmp/perry-node25-bin/node
elif [ -x /home/github-runner/actions-runner/externals/node24/bin/node ]; then
  NODE=/home/github-runner/actions-runner/externals/node24/bin/node
else
  NODE="$(command -v node || true)"
fi
if [ -z "$NODE" ] || [ ! -x "$NODE" ]; then
  echo "Node binary not found" >&2
  exit 1
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

"$NODE" --input-type=module - > "$TMPDIR/node-constants.json" <<'NODE'
import constantsDefault from "node:constants";
import * as constantsNs from "node:constants";

const defaultKeys = Object.keys(constantsDefault).sort();
const namespaceKeys = Object.keys(constantsNs)
  .filter((name) => name !== "default")
  .sort();

console.log(JSON.stringify({
  version: process.version,
  platform: process.platform,
  defaultKeys,
  namespaceKeys,
  defaultOnly: defaultKeys.filter((name) => !namespaceKeys.includes(name)),
  namespaceOnly: namespaceKeys.filter((name) => !defaultKeys.includes(name)),
}));
NODE

"$PERRY" --print-api-manifest=json > "$TMPDIR/perry-manifest.json"

"$NODE" --input-type=module - "$TMPDIR/node-constants.json" "$TMPDIR/perry-manifest.json" <<'NODE'
import fs from "node:fs";

const [nodePath, manifestPath] = process.argv.slice(2);
const nodeInfo = JSON.parse(fs.readFileSync(nodePath, "utf8"));
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const entries = Array.isArray(manifest) ? manifest : manifest.entries;
const manifestKeys = [...new Set(
  entries
    .filter((entry) => entry.module === "constants" || entry.module === "node:constants")
    .map((entry) => entry.name),
)].sort();

const missing = nodeInfo.namespaceKeys.filter((name) => !manifestKeys.includes(name));
const extra = manifestKeys.filter((name) => !nodeInfo.namespaceKeys.includes(name));

if (nodeInfo.defaultOnly.length || nodeInfo.namespaceOnly.length || missing.length || extra.length) {
  console.error(JSON.stringify({
    nodeVersion: nodeInfo.version,
    platform: nodeInfo.platform,
    nodeDefaultOnly: nodeInfo.defaultOnly,
    nodeNamespaceOnly: nodeInfo.namespaceOnly,
    manifestMissing: missing,
    manifestExtra: extra,
  }, null, 2));
  process.exit(1);
}

console.log(`node constants namespace: ${nodeInfo.version} ${nodeInfo.platform} ${nodeInfo.namespaceKeys.length}`);
console.log(`perry constants manifest: ${manifestKeys.length}`);
console.log(
  `RTLD_DEEPBIND present: ${nodeInfo.namespaceKeys.includes("RTLD_DEEPBIND")} ${manifestKeys.includes("RTLD_DEEPBIND")}`,
);
NODE
