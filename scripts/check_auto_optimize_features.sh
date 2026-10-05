#!/usr/bin/env bash
# Check pruned stdlib configurations without linking or allowing Perry's
# prebuilt fallback to turn a failed feature build into a successful test.
set -euo pipefail
cd "$(dirname "$0")/.."

# Mirrors optimized_libs/driver.rs: async-bridge is the unconditional floor;
# routing node:zlib externally removes compression and adds external-zlib-pump;
# global fetch retains web-fetch; net/HTTP routing adds the respective pumps.
# The third row is the feature set selected for upm (fetch, crypto, Web Streams
# with Brotli, and external node:zlib); web-fetch itself implies bundled-streams.
# Keep full/default features out: they hide missing optional-feature imports.
sets=(
  async-bridge,external-zlib-pump
  async-bridge
  async-bridge,bundled-streams,crypto,external-zlib-pump,streams-brotli,web-fetch
  async-bridge,external-zlib-pump,external-net-pump,external-net-tls,external-http-client-pump,external-http-server-pump,external-ws-pump
  async-bridge,compression-gzip
)
for features in "${sets[@]}"; do
  echo "Checking auto-optimize stdlib features: $features"
  cross_features="perry-runtime/full"
  IFS=, read -ra selected <<< "$features"
  for feature in "${selected[@]}"; do
    cross_features+=",perry-stdlib/$feature"
  done
  cargo check --locked -p perry-runtime-static -p perry-stdlib-static \
    --no-default-features --features "$cross_features"
done
