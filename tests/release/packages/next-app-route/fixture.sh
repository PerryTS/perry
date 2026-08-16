#!/usr/bin/env bash
# Pinned #8034 production App Route / shared-provider gate for #8036.
#
# What this asserts, on every run: Next 16.3.0's UNTOUCHED production webpack
# output compiles to an app-only dylib against separately loaded runtime and
# stdlib provider images, serves through a `dlopen` host, and matches the Node
# production oracle byte-for-byte across 10 cold starts of two 21-request
# verifier passes each.
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

  BASE_URL="http://127.0.0.1:$port" node verify.mjs >>"$log" 2>&1 || fail "$mode cold verifier 1 failed"
  BASE_URL="http://127.0.0.1:$port" node verify.mjs >>"$log" 2>&1 || fail "$mode warm verifier 2 failed"
  if [[ "$mode" == "forced" ]]; then
    python3 "$REPO_ROOT/scripts/gc_evacuation_liveness_assert.py" \
      "$log" --probe "$NAME-$mode-$index" \
      || fail "$mode cold start $index did not prove moving-GC liveness"
  fi
  if grep -Eiq '\[perry-gc\].*SKIPPED|unsettled-await|unimplemented|compatibility[- ]fallback' "$log"; then
    fail "$mode cold start $index emitted a forbidden fallback diagnostic"
  fi
  cleanup_server
}

# One process, many verifier passes — the instrument the cold-start loop is
# structurally blind to (#8163).
#
# `run_cold_start` makes TWO verifier passes per process, and in NORMAL mode two
# passes run **zero copying minors** — measured, and the reason this arm asserts
# its own collection count below. So the cold-start loop's normal arm cannot
# exercise a moving-GC holder at all; only its forced arm moves anything.
#
# The residual #8163 failure is a holder that survives the DEFAULT (unforced)
# collector and costs roughly one broken request per hundred verifier passes in
# a warm process. Two passes cannot see it. Neither can a hundred: at p = 0.01 a
# clean 100-pass run happens ~37% of the time on a KNOWN-BROKEN build, and two
# of five measured 100-pass runs did come back clean while the bug was present.
# A green short run is therefore not evidence, which is exactly the failure mode
# CLAUDE.md warns about — a gate that cannot fail.
#
# So the arm prints the confidence its N actually buys rather than letting any
# green run imply elimination. Rule of three: ~3/p passes to be 95% confident an
# event of rate p is absent.
#
#   PERRY_NEXT_ROUTE_WARM_PASSES=300   # 95% confident vs the measured ~1/100
#   PERRY_NEXT_ROUTE_WARM_PASSES=460   # 99%
#
# Those percentages are a CONSERVATIVE FLOOR, not a model: they assume the ~1/100
# failure is independent per pass, and it is not. The failures track COLLECTIONS,
# and collections accelerate as the heap grows. Measured over one 100-pass run,
# copying minors landed at passes
#
#   3 5 11 19 28 36 43 49 54 59 64 68 72 76 79 82 86 89 92 94 97 99
#
# — early gaps of 8-9 passes tightening to 2-3 by the end (22 minors in 100
# passes; the growth is #8213's retention). So doubling N more than doubles the
# collections it buys, and the printed confidence understates a long run.
#
# The same arithmetic says what a per-cold-start pass count buys, which is a
# different thing and worth not confusing (`PERRY_NEXT_ROUTE_VERIFIERS_PER_START`,
# if present, drives THAT): a FRESH process reaches its first minor around pass 3
# and its second around pass 5, so ten passes per start is ~2 collections, and ten
# such starts ~20 — genuinely sensitive to a per-collection bug (this residual was
# caught at pass 6 of a 10-pass process, with 2 minors), but always in the
# small-heap regime. One warm process is what reaches the grown-heap regime where
# collections are frequent. Cold starts buy ABI/parity/bypass-guard coverage
# across restarts; the warm soak buys collection depth. Neither substitutes.
#
# OFF by default (0) because a meaningful N is slow — it is the acceptance
# instrument for closing #8163, not a per-run check. It deliberately runs the
# server in NORMAL mode: forcing evacuation would measure the arm that is
# already fixed.
run_warm_soak() {
  local passes="${PERRY_NEXT_ROUTE_WARM_PASSES:-0}"
  [[ "$passes" =~ ^[0-9]+$ ]] || fail "PERRY_NEXT_ROUTE_WARM_PASSES must be a non-negative integer"
  if [[ "$passes" == "0" ]]; then
    echo "  [warm soak] not run (#8163 residual) — set PERRY_NEXT_ROUTE_WARM_PASSES=300 for 95% confidence"
    return 0
  fi

  local port=$((PORT_BASE + 100))
  local log="$BUILD_DIR/perry-warm-soak.log"
  : >"$log"
  echo "  [warm soak] 1 warm process x $passes verifier passes (default GC)"
  env PERRY_GC_DIAG=1 PORT="$port" HOSTNAME=127.0.0.1 NODE_ENV=production \
    "$HOST_BIN" "$RUNTIME_IMAGE" "$STDLIB_IMAGE" "$APP_IMAGE" >>"$log" 2>&1 &
  SERVER_PID=$!
  local ready=0
  for _ in $(seq 1 150); do
    kill -0 "$SERVER_PID" 2>/dev/null || fail "warm soak exited during startup"
    if grep -q "PERRY_NEXT_APP_ROUTE_READY" "$log"; then ready=1; break; fi
    sleep 0.1
  done
  [[ "$ready" == "1" ]] || fail "warm soak did not become ready"

  local failed_iterations=()
  local pass
  for pass in $(seq 1 "$passes"); do
    if ! BASE_URL="http://127.0.0.1:$port" node verify.mjs >>"$log" 2>&1; then
      failed_iterations+=("$pass")
    fi
    kill -0 "$SERVER_PID" 2>/dev/null || fail "warm soak process died at pass $pass"
  done
  cleanup_server

  local minors
  minors="$(grep -c '\[gc-copy-minor\] ran' "$log" || true)"
  local type_errors
  type_errors="$(grep -c 'TypeError: value is not a function' "$log" || true)"
  echo "  [warm soak] passes=$passes failures=${#failed_iterations[@]} copying_minors=$minors host_type_errors=$type_errors"

  # Order matters: an OBSERVED failure is reported even when the liveness
  # counter looks wrong, because a real broken request outranks a complaint
  # about the instrument. Reversing these two once masked a genuine failure
  # behind "exercised nothing" while this arm was being tested.
  if (( ${#failed_iterations[@]} > 0 || type_errors > 0 )); then
    fail "warm soak: ${#failed_iterations[@]} failing pass(es) [${failed_iterations[*]}], $type_errors host TypeError(s) — #8163 residual"
  fi

  # A soak that ran no collections proves nothing about a GC holder, so a clean
  # run must still show its subject was live — the same rule the forced arm's
  # evacuation-liveness assert applies (CLAUDE.md: a gate must assert its
  # subject RAN, not merely that nothing threw).
  (( minors > 0 )) || fail "warm soak ran $passes passes with ZERO copying minors — it exercised nothing"

  # State what a green run of THIS size does and does not license.
  python3 - "$passes" <<'PY'
import math, sys
n = int(sys.argv[1])
p = 0.01  # measured residual rate: ~1 failing verifier pass in 100
conf = (1.0 - (1.0 - p) ** n) * 100.0
verdict = "sufficient to claim elimination" if conf >= 95.0 else "NOT sufficient — a broken build passes this often"
print(f"  [warm soak] clean at N={n}: {conf:.0f}% confidence the ~1/100 residual is gone ({verdict})")
PY
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
  echo "  [6/7] $COLD_STARTS cold processes (alternating normal / FORCED-evacuation), two 21-request verifier runs each"
else
  echo "  [6/7] $COLD_STARTS cold processes, two 21-request verifier runs each"
  echo "        forced-evacuation arm OFF (PERRY_NEXT_ROUTE_FORCED_GC=0)"
fi
for index in $(seq 0 $((COLD_STARTS - 1))); do
  if [[ "$FORCED_GC" == "1" ]] && (( index % 2 == 1 )); then mode="forced"; else mode="normal"; fi
  run_cold_start "$index" "$mode"
done

run_warm_soak

echo "  [7/7] production AppRouteRouteModule.handle parity complete"
if [[ "$FORCED_GC" == "1" ]]; then
  echo "PASS $NAME (with forced-evacuation arm)"
else
  echo "PASS $NAME (forced-evacuation arm not run — PERRY_NEXT_ROUTE_FORCED_GC=0)"
fi
