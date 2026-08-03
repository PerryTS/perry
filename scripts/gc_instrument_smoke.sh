#!/usr/bin/env bash
# End-to-end exercised arm for the #7154 rooting-bug instruments
# (`PERRY_GC_PROTECT_FROMSPACE`, `PERRY_GC_ZEAL`).
#
# WHY THIS EXISTS
#
# CLAUDE.md's GC knob kill-policy is binding: a knob with no arm exercising it
# is a configuration nobody has verified, and this repo has repeatedly paid for
# that (`PERRY_GC_FORCE_EVACUATE` inert for every `gc()`-driven test, #6942 /
# #6946; the matrix's `--pressure` knob disabling the path it measured, #7024).
#
# The *detection* property — "a planted stale from-space deref is caught" — is
# asserted as a unit test in the required `cargo-test` gate
# (`gc/tests/fromspace_protect.rs::quarantine_catches_a_planted_stale_from_space_deref`).
# What a unit test cannot cover is the INTEGRATED path: codegen actually
# emitting back-edge polls, zeal actually firing on them, the copying minor
# actually running, and the quarantine actually retiring its from-space in a
# real compiled program. That is this script.
#
# NON-VACUITY IS THE POINT. Per CLAUDE.md's "four ways a gate can be unable to
# fail" #4, a gate must assert its subject was live. A protected run with zero
# copying minors protects nothing and would pass silently. So this script does
# not merely check the program's output: it requires the zeal arm to produce
# strictly MORE quarantine retirements than the no-zeal arm, which can only
# happen if zeal genuinely forced collections that pressure would not have.
#
# Usage: scripts/gc_instrument_smoke.sh [path-to-perry]
#   Expects target/release/perry and PERRY_RUNTIME_DIR-resolvable staticlibs.

set -euo pipefail

PERRY_BIN="${1:-target/release/perry}"
if [[ ! -x "$PERRY_BIN" ]]; then
  echo "FAIL: no perry binary at $PERRY_BIN" >&2
  exit 1
fi
PERRY_BIN="$(cd "$(dirname "$PERRY_BIN")" && pwd)/$(basename "$PERRY_BIN")"
export PERRY_RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$(dirname "$PERRY_BIN")}"
export PERRY_NO_AUTO_OPTIMIZE=1

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# A deliberately SMALL correct program that still exercises the whole path:
# a constructor that allocates inside a loop (so codegen emits back-edge polls
# and the instance survives a collection inside the callee — the #7192 shape),
# called in an outer loop, with the caller reading a field back afterwards so a
# stale read cannot go unnoticed. Sized for ~1200 polls, not #7154's 240k, so
# the zeal arm costs seconds rather than minutes.
cat > "$WORK/fixture.ts" <<'TS'
class Holder {
  payload: any;
  constructor(n: number) {
    const bits: any[] = [];
    for (let i = 0; i < 40; i++) {
      bits.push({ i: i, s: "x" });
    }
    this.payload = { n: n, len: bits.length };
  }
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 30; r++) {
    const h = new Holder(r);
    const p = h.payload;
    if (p === null || p === undefined) {
      bad++;
    } else if ((p.n as number) !== r || (p.len as number) !== 40) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
TS

echo "== compiling fixture with PERRY_GC_MOVING_LOOP_POLLS=1 (zeal needs the polls) =="
PERRY_GC_MOVING_LOOP_POLLS=1 "$PERRY_BIN" compile "$WORK/fixture.ts" -o "$WORK/fixture" >/dev/null

# $1 = label, rest = env assignments. Echoes the retirement count.
run_arm() {
  local label="$1"; shift
  local out rc retired
  set +e
  out="$(env "$@" PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_DIAG=1 "$WORK/fixture" 2>&1)"
  rc=$?
  set -e
  retired="$(grep -c 'gc-fromspace-protect. mode=' <<<"$out" || true)"
  if [[ $rc -ne 0 ]]; then
    echo "FAIL [$label]: exited $rc" >&2
    echo "$out" | tail -30 >&2
    exit 1
  fi
  if ! grep -q '^bad 0$' <<<"$out"; then
    echo "FAIL [$label]: expected 'bad 0', got:" >&2
    grep '^bad' <<<"$out" >&2 || echo "(no 'bad' line)" >&2
    exit 1
  fi
  echo "  [$label] correct output, exit 0, quarantine retirements=$retired"
  echo "$retired"
}

echo "== arm 1: instruments OFF (baseline correctness) =="
off_retired="$(run_arm off | tail -1)"
if [[ "$off_retired" -ne 0 ]]; then
  echo "FAIL: the instrument retired $off_retired page-sets with the knob OFF." >&2
  echo "      Default-off must mean inert." >&2
  exit 1
fi

echo "== arm 2: PROTECT_FROMSPACE=1 without zeal (pressure-only) =="
nozeal_retired="$(run_arm protect PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 | tail -1)"

echo "== arm 3: PROTECT_FROMSPACE=1 + ZEAL=1 (the investigation pairing) =="
zeal_retired="$(run_arm protect+zeal PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 | tail -1)"

# ---- non-vacuity gate -------------------------------------------------------
# The subject must have been LIVE. Without this, every arm above could pass
# having run zero copying minors — the exact failure mode #6942/#7024/#7025
# were filed for.
if [[ "$zeal_retired" -eq 0 ]]; then
  echo "FAIL: zeal + protection retired ZERO from-space page-sets." >&2
  echo "      The instruments did not run, so a clean result proves nothing." >&2
  echo "      Most likely: codegen emitted no back-edge polls, or the copying" >&2
  echo "      minor was ineligible (conservative stack scan / pinned young)." >&2
  exit 1
fi
if [[ "$zeal_retired" -le "$nozeal_retired" ]]; then
  echo "FAIL: zeal did not force any additional collection" >&2
  echo "      (no-zeal=$nozeal_retired, zeal=$zeal_retired)." >&2
  echo "      PERRY_GC_ZEAL is inert on this build — it must collect at" >&2
  echo "      safepoints where no trigger is due." >&2
  exit 1
fi

# ---- arm 4: the quarantine, aimed at real programs --------------------------
#
# #7341. Everything above drives the instrument with PERRY_GC_MOVING_LOOP_POLLS
# over ONE synthetic fixture. Back-edge polls fire only while user JS runs, so
# that configuration structurally CANNOT expose an unrooted pointer in runtime
# code that does not re-enter user JS -- which is most of the runtime. The gate
# was well built and pointed at the wrong workload; aiming it at the gc_ratchet
# probes by the ALLOCATION-POINT route instead found 55 stale from-space
# dereferences across the gap suite, 44 of them in programs that exit cleanly
# and print the right answer.
#
# It has to be a fault-based check, not an output check: evacuation copies
# rather than zeroes, so a stale address still reads the correct old bytes.
# Only unmapping retired from-space turns the latent access into a signal.
PROBES="$(dirname "$0")/../benchmarks/gc_ratchet/probes"
if [[ -d "$PROBES" ]]; then
  echo
  echo "== arm 4: quarantine over the gc_ratchet probes (allocation-point route) =="
  probe_count=0
  probe_failed=0
  for probe in "$PROBES"/*.ts; do
    [[ -e "$probe" ]] || continue
    name="$(basename "$probe" .ts)"
    probe_count=$((probe_count + 1))
    "$PERRY_BIN" compile "$probe" -o "$WORK/$name" >/dev/null
    set +e
    PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
    PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off \
      "$WORK/$name" > "$WORK/$name.out" 2> "$WORK/$name.err"
    rc=$?
    set -e
    if [[ $rc -ge 128 ]]; then
      echo "FAIL [$name]: signalled $rc under from-space quarantine." >&2
      echo "      A live value still pointed into retired from-space after an" >&2
      echo "      evacuating minor. The handler's diagnosis:" >&2
      grep -A6 'gc-fromspace-protect. FAULT' "$WORK/$name.err" >&2 || true
      probe_failed=$((probe_failed + 1))
    fi
  done
  # Non-vacuity: the arm must have had a subject. A probe set that silently
  # stopped matching would otherwise report a clean sweep of nothing -- the
  # exact shape of #6942 / #7024 / #7336.
  if [[ "$probe_count" -eq 0 ]]; then
    echo "FAIL: no probes matched $PROBES -- arm 4 ran on nothing." >&2
    exit 1
  fi
  if [[ "$probe_failed" -ne 0 ]]; then
    echo "FAIL: $probe_failed/$probe_count probes faulted under the quarantine." >&2
    exit 1
  fi
  echo "  $probe_count/$probe_count probes clean under from-space quarantine"
fi

echo
echo "PASS: instruments inert when off (0 retirements), live when on"
echo "      (no-zeal=$nozeal_retired, zeal=$zeal_retired retirements), program correct in all arms."
echo "      Quarantine clean over $probe_count real probes (allocation-point route)."
