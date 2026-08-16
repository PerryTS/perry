#!/usr/bin/env bash
# Pinned #8034 production App Route / shared-provider gate for #8036.
#
# What this asserts, on every run: Next 16.3.0's UNTOUCHED production webpack
# output compiles to an app-only dylib against separately loaded runtime and
# stdlib provider images, serves through a `dlopen` host, and matches the Node
# production oracle byte-for-byte across 10 cold starts of
# `PERRY_NEXT_ROUTE_VERIFIERS_PER_START` (default 10) 21-request verifier
# passes each — 100 batches by default, #8040's "100-iteration run of the
# 20-way concurrent request batch". Every request must also enter the generated
# `AppRouteRouteModule.handle`: `perry-host.js` wraps it and logs
# `generated handler bypassed routeModule.handle` for any request that reached
# the userland handler another way, and each cold-start log is grepped for that
# line as a hard failure. That signal lives ONLY in the host log — the guard
# throws inside a `.then()` after the response is already sent, so
# `verify.mjs` still exits 0 when it fires (#8161).
#
# Known state (#8163): with the default 10 verifier passes per process this
# fixture is intermittently RED on today's `main` — a default-mode copying
# minor occasionally strands a stale closure, and ~2% of the batches that
# follow one lose a response (`TypeError: value is not a function` in the host
# log right after a `[gc-copy-minor] ran` line, then an empty body in
# `verify.mjs`). Two passes per process finished before the first copying
# minor (~pass 3), which is how the fixture read green while #8040's
# 100-iteration bullet was red. Set `PERRY_NEXT_ROUTE_VERIFIERS_PER_START=2`
# to recover the old coverage.
#
# Odd cold starts run under FORCED evacuation with a seeded GC schedule and the
# moving-GC liveness assert (#8163 — fixed; `PERRY_NEXT_ROUTE_FORCED_GC=0`
# turns that arm off for a normal-only run).
set -euo pipefail
cd "$(dirname "$0")"

NAME="next-app-route"
REPO_ROOT="$(cd ../../../.. && pwd)"
PERRY_BIN="${PERRY_BIN:-$REPO_ROOT/target/release/perry}"
PORT_BASE="${PERRY_NEXT_ROUTE_PORT:-31836}"
COLD_STARTS="${PERRY_NEXT_ROUTE_COLD_STARTS:-10}"
# Verifier passes per cold start: restart/ABI/parity/bypass-guard coverage
# across N fresh processes. Each 10-pass process runs ~2-3 copying minors
# (the first lands around pass 3), so this arm is sensitive to per-collection
# bugs — but a fresh process lives permanently in the early/small-heap regime
# and never reaches the grown heap where collections accelerate. Collection
# DEPTH in one process is a different knob (`PERRY_NEXT_ROUTE_WARM_PASSES`,
# #8215); neither substitutes for the other.
VERIFIERS_PER_START="${PERRY_NEXT_ROUTE_VERIFIERS_PER_START:-10}"
if [[ -n "${PERRY_NEXT_ROUTE_BUILD_DIR:-}" ]]; then
  BUILD_DIR="$PERRY_NEXT_ROUTE_BUILD_DIR"
  BUILD_DIR_OWNED=0
  mkdir -p "$BUILD_DIR"
else
  BUILD_DIR="$(mktemp -d)"
  BUILD_DIR_OWNED=1
fi
KEEP_BUILD="${PERRY_NEXT_ROUTE_KEEP_BUILD:-0}"
SERVER_PID=""
TS_CONFIG_BACKUP="$BUILD_DIR/tsconfig.json"

cleanup_server() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  SERVER_PID=""
}

cleanup() {
  cleanup_server
  if [[ -f "$TS_CONFIG_BACKUP" ]]; then
    cp "$TS_CONFIG_BACKUP" tsconfig.json
  fi
  if [[ "$BUILD_DIR_OWNED" == "1" && "$KEEP_BUILD" != "1" ]]; then
    rm -rf "$BUILD_DIR"
  elif [[ "$KEEP_BUILD" == "1" ]]; then
    echo "  kept build artifacts at $BUILD_DIR"
  fi
}
trap cleanup EXIT

fail() {
  echo "FAIL $NAME — $1"
  [[ -f "$BUILD_DIR/perry-run.log" ]] && tail -80 "$BUILD_DIR/perry-run.log" | sed 's/^/    /'
  exit 1
}

for tool in npm node cargo cc ar nm python3; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is not on PATH"
done
for value in "$COLD_STARTS" "$VERIFIERS_PER_START"; do
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || fail "cold starts and verifiers per start must be positive integers (got '$value')"
done
TOTAL_BATCHES=$((COLD_STARTS * VERIFIERS_PER_START))
# `generated handler bypassed` is the routeModule.handle guard in perry-host.js.
# It is a log line, not an exit code: verify.mjs passes even when it fires, so
# the per-cold-start grep below is the only place the guard can fail the run.
FORBIDDEN_DIAGNOSTICS='generated handler bypassed|\[perry-gc\].*SKIPPED|unsettled-await|unimplemented|compatibility[- ]fallback'
[[ -x "$PERRY_BIN" ]] || fail "perry not found at $PERRY_BIN"

case "$(uname -s)" in
  Darwin) SHARED_EXT="dylib" ;;
  Linux) SHARED_EXT="so" ;;
  *) echo "SKIP $NAME — shared-provider host currently requires dlopen and ar"; exit 0 ;;
esac

cp tsconfig.json "$TS_CONFIG_BACKUP"

echo "  [1/7] install and production webpack build (Next 16.3.0)"
npm ci --silent --no-audit --no-fund >"$BUILD_DIR/npm-install.log" 2>&1
npm run build >"$BUILD_DIR/next-build.log" 2>&1
ROUTE_BUNDLE=".next/server/app/api/benchmark/route.js"
[[ -f "$ROUTE_BUNDLE" ]] || fail "production route bundle was not generated"
grep -q "AppRouteRouteModule" "$ROUTE_BUNDLE" || fail "route bundle lacks AppRouteRouteModule"
grep -qE '\.handle\(' "$ROUTE_BUNDLE" || fail "route bundle lacks routeModule.handle"

echo "  [2/7] exact Node production oracle"
: >"$BUILD_DIR/node-oracle.log"
PORT="$PORT_BASE" npm start >>"$BUILD_DIR/node-oracle.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 100); do
  kill -0 "$SERVER_PID" 2>/dev/null || fail "Node oracle exited during startup"
  if BASE_URL="http://127.0.0.1:$PORT_BASE" node verify.mjs >>"$BUILD_DIR/node-oracle.log" 2>&1; then
    break
  fi
  sleep 0.1
done
grep -q "PASS: 21 production App Route requests" "$BUILD_DIR/node-oracle.log" || fail "Node oracle verifier failed"
cleanup_server

echo "  [3/7] build coherent runtime and stdlib provider archives"
CARGO_TARGET_DIR="$BUILD_DIR/provider-target" cargo build \
  --manifest-path provider/Cargo.toml --release \
  -p perry-next-runtime-provider -p perry-next-stdlib-provider \
  >"$BUILD_DIR/provider-build.log" 2>&1
RUNTIME_ARCHIVE="$BUILD_DIR/provider-target/release/libperry_next_runtime_provider.a"
STDLIB_ARCHIVE="$BUILD_DIR/provider-target/release/libperry_next_stdlib_provider.a"
[[ -f "$RUNTIME_ARCHIVE" && -f "$STDLIB_ARCHIVE" ]] || fail "provider archives were not produced"

# perry-stdlib is an rlib dependency of the umbrella archive and therefore
# carries a copy of perry-runtime. Remove only those runtime codegen members;
# the separately loaded runtime image is the single owner of GC/event state.
TRIMMED_STDLIB="$BUILD_DIR/libperry_next_stdlib_provider.trimmed.a"
cp "$STDLIB_ARCHIVE" "$TRIMMED_STDLIB"
while IFS= read -r member; do
  [[ -n "$member" ]] && ar -d "$TRIMMED_STDLIB" "$member"
done < <(ar -t "$TRIMMED_STDLIB" | grep '^perry_runtime-' || true)
if nm -g "$TRIMMED_STDLIB" 2>/dev/null | grep -qE ' [Tt] _?js_gc_init$'; then
  fail "stdlib provider still owns runtime ABI symbols"
fi

echo "  [4/7] link separate provider images and dlopen host"
RUNTIME_IMAGE="$BUILD_DIR/libperry_runtime_provider.$SHARED_EXT"
STDLIB_IMAGE="$BUILD_DIR/libperry_stdlib_provider.$SHARED_EXT"
HOST_BIN="$BUILD_DIR/provider-host"
if [[ "$SHARED_EXT" == "dylib" ]]; then
  mac_libs=(-framework Security -framework CoreFoundation -framework SystemConfiguration -liconv -lresolv -lobjc)
  cc -dynamiclib -Wl,-force_load,"$RUNTIME_ARCHIVE" -Wl,-undefined,dynamic_lookup "${mac_libs[@]}" -o "$RUNTIME_IMAGE"
  cc -dynamiclib -Wl,-force_load,"$TRIMMED_STDLIB" -Wl,-undefined,dynamic_lookup "${mac_libs[@]}" -o "$STDLIB_IMAGE"
else
  linux_libs=(-lm -lpthread -ldl -lssl -lcrypto)
  cc -shared -Wl,--whole-archive "$RUNTIME_ARCHIVE" -Wl,--no-whole-archive -Wl,--allow-shlib-undefined "${linux_libs[@]}" -o "$RUNTIME_IMAGE"
  cc -shared -Wl,--whole-archive "$TRIMMED_STDLIB" -Wl,--no-whole-archive -Wl,--allow-shlib-undefined "${linux_libs[@]}" -o "$STDLIB_IMAGE"
fi
cc provider-host.c -ldl -o "$HOST_BIN"

echo "  [5/7] compile untouched production route handler as app-only dylib"
APP_IMAGE="$BUILD_DIR/next-app.$SHARED_EXT"
PERRY_RUNTIME_DIR="$(dirname "$RUNTIME_ARCHIVE")" \
  "$PERRY_BIN" compile perry-host.js --output-type dylib --no-auto-optimize --no-cache \
  -o "$APP_IMAGE" >"$BUILD_DIR/perry-compile.log" 2>&1 || fail "Perry dylib compile failed"
nm -u "$APP_IMAGE" 2>/dev/null | grep -qE '_?js_gc_init$' || fail "app dylib unexpectedly embeds the Perry ABI"

PERRY_COMMIT="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
if command -v shasum >/dev/null 2>&1; then
  PROVIDER_ABI_HASH="$(shasum -a 256 "$RUNTIME_IMAGE" "$STDLIB_IMAGE" | shasum -a 256 | awk '{print $1}')"
else
  PROVIDER_ABI_HASH="$(sha256sum "$RUNTIME_IMAGE" "$STDLIB_IMAGE" | sha256sum | awk '{print $1}')"
fi
echo "  commit=$PERRY_COMMIT next=16.3.0 mode=dylib providers=$PROVIDER_ABI_HASH"

run_cold_start() {
  local index="$1"
  local mode="$2"
  local port=$((PORT_BASE + index + 1))
  local log="$BUILD_DIR/perry-${mode}-${index}.log"
  : >"$log"
  if [[ "$mode" == "forced" ]]; then
    # A forced-evacuation flag proves nothing unless the process actually runs
    # a copying minor and moves a live object. Keep the production request
    # workload deterministic under a small heap, disable the two known
    # non-moving fallbacks, and emit the diagnostics consumed by the repository's
    # evacuation-liveness checker below.
    env PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 \
      PERRY_GC_DIAG=1 PERRY_GC_TRACE=1 PERRY_GC_HEAP_LIMIT=8 \
      PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off \
      PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=8036 \
      PORT="$port" HOSTNAME=127.0.0.1 NODE_ENV=production \
      "$HOST_BIN" "$RUNTIME_IMAGE" "$STDLIB_IMAGE" "$APP_IMAGE" >>"$log" 2>&1 &
  else
    env PORT="$port" HOSTNAME=127.0.0.1 NODE_ENV=production \
      "$HOST_BIN" "$RUNTIME_IMAGE" "$STDLIB_IMAGE" "$APP_IMAGE" >>"$log" 2>&1 &
  fi
  SERVER_PID=$!
  local ready=0
  for _ in $(seq 1 150); do
    kill -0 "$SERVER_PID" 2>/dev/null || fail "$mode cold start $index exited during startup"
    if grep -q "PERRY_NEXT_APP_ROUTE_READY" "$log"; then ready=1; break; fi
    sleep 0.1
  done
  [[ "$ready" == "1" ]] || fail "$mode cold start $index did not become ready"

  local verifier label
  for verifier in $(seq 1 "$VERIFIERS_PER_START"); do
    if (( verifier == 1 )); then label="cold"; else label="warm"; fi
    BASE_URL="http://127.0.0.1:$port" node verify.mjs >>"$log" 2>&1 || {
      tail -40 "$log" | sed 's/^/    /'
      fail "$mode cold start $index: $label verifier $verifier/$VERIFIERS_PER_START failed"
    }
  done
  if [[ "$mode" == "forced" ]]; then
    python3 "$REPO_ROOT/scripts/gc_evacuation_liveness_assert.py" \
      "$log" --probe "$NAME-$mode-$index" \
      || fail "$mode cold start $index did not prove moving-GC liveness"
  fi
  if grep -Eiq "$FORBIDDEN_DIAGNOSTICS" "$log"; then
    grep -Ein "$FORBIDDEN_DIAGNOSTICS" "$log" | head -20 | sed 's/^/    /'
    fail "$mode cold start $index emitted a forbidden fallback diagnostic"
  fi
  cleanup_server
}

# The forced-evacuation arm is ON by default: odd cold starts run under forced
# evacuation with the moving-GC liveness assert. It was opt-in (and red) while
# #8163 was open — a stale closure reached Next's `Reflect.get` adapter from
# `HEADERS_METHOD_VALUE_CACHE`, and a second one reached the `res.end()` tail
# from `ServerResponse.once_listeners`; both were holders outside the GC heap
# that no root scanner marked or rewrote. `PERRY_NEXT_ROUTE_FORCED_GC=0` runs
# the normal arm only; it is a knob, not a skip, and never `continue-on-error`.
FORCED_GC="${PERRY_NEXT_ROUTE_FORCED_GC:-1}"
if [[ "$FORCED_GC" == "1" ]]; then
  echo "  [6/7] $COLD_STARTS cold processes (alternating normal / FORCED-evacuation), $VERIFIERS_PER_START 21-request verifier runs each ($TOTAL_BATCHES batches)"
else
  echo "  [6/7] $COLD_STARTS cold processes, $VERIFIERS_PER_START 21-request verifier runs each ($TOTAL_BATCHES batches)"
  echo "        forced-evacuation arm OFF (PERRY_NEXT_ROUTE_FORCED_GC=0)"
fi
for index in $(seq 0 $((COLD_STARTS - 1))); do
  if [[ "$FORCED_GC" == "1" ]] && (( index % 2 == 1 )); then mode="forced"; else mode="normal"; fi
  run_cold_start "$index" "$mode"
done

echo "  [7/7] production AppRouteRouteModule.handle parity complete: $TOTAL_BATCHES verifier batches over $COLD_STARTS cold starts, 0 bypass-guard fires"
if [[ "$FORCED_GC" == "1" ]]; then
  echo "PASS $NAME ($TOTAL_BATCHES batches, with forced-evacuation arm)"
else
  echo "PASS $NAME ($TOTAL_BATCHES batches, forced-evacuation arm not run — PERRY_NEXT_ROUTE_FORCED_GC=0)"
fi
