#!/bin/bash
# scripts/a1/irdiff.sh <perry-main> <perry-head> <outdir> [allow.txt] [--full]
#
# Exact per-function Ir diff of startup witnesses between two perry builds (A1 design 4.3,
# nightly/CW/STARTUP_RULE_2026-10-28.md). For each witness and arm: compile with
# PERRY_TARGET_CPU=x86-64-v3 PERRY_KEEP_SYMBOLS=1 to the SAME output path from the SAME cwd,
# run 3 times under `setarch -R valgrind --tool=callgrind` (totals must be identical),
# callgrind_annotate per function, join by function name.
#
# Writes <outdir>/<witness>.tsv (fn, ir_main, ir_head, delta; delta descending) and
# <outdir>/summary.tsv (witness, total_main, total_head, delta, functions_with_plus_ir).
# Exit 1 if a witness total is above main, or a function with +Ir is not listed in allow.txt
# (one function name per line, '#' comments). A matched +/- pair from inlining is explained by
# listing both names there.
# Measurement controls (preload chain below: fixed inputs, maps shim, byte-loop string compares) follow the accepted JST recipe (nightly/CW/STARTUP_RULE_2026-10-28.md): a minimal
# environment, the fixed-inputs preload (clock, entropy, /proc/self/maps normalization), ASLR off,
# --cache-sim=no --branch-sim=no. IRDIFF_PRELOAD overrides the preload path.
# IRDIFF_ONLY="hello commander_help" limits the run to those witnesses.
# The startup-path ahash maps seed from one per-process key block with no address mixed in
# (fast_hash::AHashState); the fixed-inputs preload pins the entropy, so counts are layout-independent.
# Each perry binary sits next to its runtime libs (PERRY_RUNTIME_DIR = dirname of the resolved
# binary). Workspace roots: IRDIFF_WS_MAIN / IRDIFF_WS_HEAD (defaults /root/A1S0/main-src and
# /root/A1S0/src (arms need equal-length roots)). Witness roots: IRDIFF_SCORE (/root/hwp-move/score), IRDIFF_SAMPLES (/root/STU/samples),
# IRDIFF_NSB (/root/NSB/jsc).
set -u
full=0; pos=()
for a in "$@"; do [ "$a" = --full ] && full=1 || pos+=("$a"); done
MAIN=$(readlink -f "${pos[0]:?perry-main}"); HEAD=$(readlink -f "${pos[1]:?perry-head}"); OUT=${pos[2]:?outdir}
ALLOW=${pos[3]:-/dev/null}
SC=${IRDIFF_SCORE:-/root/hwp-move/score}; SAMPLES=${IRDIFF_SAMPLES:-/root/STU/samples}; NSB=${IRDIFF_NSB:-/root/NSB/jsc}
WS_MAIN=${IRDIFF_WS_MAIN:-/root/A1S0/main-src}; WS_HEAD=${IRDIFF_WS_HEAD:-/root/A1S0/src}
mkdir -p "$OUT/w" "$OUT/cg" "$OUT/tmp" "$OUT/pad"; OUT=$(readlink -f "$OUT")
# Artifact controls (STARTUP_RULE_2026-10-28.md, addendum), applied to BOTH arms: the fixed-inputs preload
# (clock, entropy, /proc/self/maps normalization), JAG10b's maps shim (std's stack-guard parse of
# /proc/self/maps answers the main thread from the stack rlimit, so valgrind's mapping placement per arm
# costs nothing) and the byte-loop string control (memcmp, bcmp, strcmp, strncmp, getenv, secure_getenv:
# glibc avx2 compare cost follows rodata alignment). Each is overridable; all three must exist.
FIXED=${IRDIFF_PRELOAD:-/root/JAG10/fixed-inputs.so}
MAPS_SHIM=${IRDIFF_MAPS_SHIM:-/root/JAG10/tmp/stk-shim.so}
STR_PLAIN=${IRDIFF_STR_PLAIN:-/root/IOV/ctl/str-plain.so}
for f in "$FIXED" "$MAPS_SHIM" "$STR_PLAIN"; do [ -f "$f" ] || { echo "missing measurement preload $f" >&2; exit 2; }; done
PRELOAD="$FIXED:$MAPS_SHIM:$STR_PLAIN"
# Link-pad control (on unless IRDIFF_LINKPAD=0): rows that follow addresses with equal call counts (mimalloc
# page map, address-sorted ranges, libc alignment) move when one arm's sections are smaller. linkpad.py pads
# each arm with unreferenced retained sections linked last (PERRY_EXTRA_LINK_ARGS) until every paddable
# section ends at the same address in both arms, so later sections, segments and the heap start coincide;
# the layout proof is $OUT/<witness>.layout.txt and the pads are $OUT/pads.tsv. Nothing executes a pad.
# Self-test of the control (A/A): IRDIFF_HEAD_LINK_ARGS links extra inputs (e.g. a perturbing pad object)
# into the head arm only; with the same perry on both arms the unpadded run shows the address rows, the
# padded run must show 0.
LINKPAD=${IRDIFF_LINKPAD:-1}; LINKPAD_PY=$(dirname "$(readlink -f "$0")")/linkpad.py
# minimal environment of both the compile and the run (same for both arms)
ENVV=(PATH=/usr/lib/llvm-22/bin:/root/.cargo/bin:/usr/bin:/bin HOME="$HOME" LANG=C LC_ALL=C TZ=UTC LD_PRELOAD="$PRELOAD" PERRY_ALLOW_PERRY_FEATURES=1 TMPDIR="$OUT/tmp" NO_COLOR=1 PKG_BENCH_N=0 PKG_BENCH_WARM=0)

# name|dir|source|args  (args are run as given; dir is the cwd of both compile and run)
W=("hello|$SAMPLES|01-hello/main.ts|Perry"
   "commander_help|$SC/p/pk|commander/parse_argv.ts|--help"
   "commander|$SC/p/pk|commander/parse_argv.ts|"
   "fastify_inject|$SC/p/pk|fastify/inject.ts|"
   "awfy_richards|$SC/p/awfy|harness.js|Richards 1 0"
   "binary_trees|$NSB|benches/23-binary-trees.ts|")
if [ $full = 1 ]; then
  W+=("tsc_tssrc|$SC/p/tssrc/work|tssrc.ts|3"
      "claude_code_newest|${IRDIFF_CLAUDE:-/root/secret-tests/claude-code-newest}|${IRDIFF_CLAUDE_SRC:-unbun-out/main.js}|--version"
      "opencode_target|${IRDIFF_OPENCODE:-/root/secret-tests/opencode-target}|${IRDIFF_OPENCODE_SRC:-split/main.js}|--version"
      "nextjs_hello|${IRDIFF_NEXT:-/root/secret-tests/nextjs-hello}|${IRDIFF_NEXT_SRC:-main.ts}|")
fi

compile() { # arm perry ws name dir src [pad-object]  -> $OUT/w/$n.$arm
  local arm=$1 perry=$2 ws=$3 n=$4 d=$5 s=$6 pad=${7:--} extra=() link=""
  [ "$arm" = head ] && link=${IRDIFF_HEAD_LINK_ARGS:-}
  [ "$pad" != - ] && link="$link $pad"
  [ -n "${link// }" ] && extra=(PERRY_EXTRA_LINK_ARGS="${link# }")
  ( cd "$d" && env -i "${ENVV[@]}" "${extra[@]}" PERRY_RUNTIME_DIR=$(dirname "$perry") PERRY_WORKSPACE_ROOT=$ws PERRY_TARGET_CPU=x86-64-v3 PERRY_KEEP_SYMBOLS=1 \
      "$perry" compile --cache-dir "$OUT/tmp/cache" "$s" -o "$OUT/w/$n" > "$OUT/cg/$n.$arm.compile.log" 2>&1 )
  local rc=$?; rm -rf "$OUT/tmp/cache"; [ $rc = 0 ] && mv "$OUT/w/$n" "$OUT/w/$n.$arm"; return $rc
}
# pads both arms of witness $n to the same layout (see LINKPAD); exit 1 if it does not converge
linkpad() { # name dir src
  local n=$1 d=$2 s=$3 r res pm=- ph=- cm=- ch=-
  rm -f "$OUT/pad/$n.json"
  for r in 1 2 3 4 5; do
    res=$(python3 "$LINKPAD_PY" plan "$OUT/w/$n.main" "$OUT/w/$n.head" "$OUT/pad/$n.json" "$OUT/pad") || return 1
    [ "$res" = CONVERGED ] && break
    pm=$(sed 's/.*main=\([^ ]*\).*/\1/' <<< "$res"); ph=$(sed 's/.*head=\([^ ]*\).*/\1/' <<< "$res")
    [ "$pm" != "$cm" ] && { compile main "$MAIN" "$WS_MAIN" "$n" "$d" "$s" "$pm" || return 1; cm=$pm; }
    [ "$ph" != "$ch" ] && { compile head "$HEAD" "$WS_HEAD" "$n" "$d" "$s" "$ph" || return 1; ch=$ph; }
  done
  printf "%s\t%s\t%s\t%s\t%s\n" "$n" "$res" "$(basename "$cm")" "$(basename "$ch")" "$(tr -d ' \n' < "$OUT/pad/$n.json")" >> "$OUT/pads.tsv"
  python3 "$LINKPAD_PY" check "$OUT/w/$n.main" "$OUT/w/$n.head" > "$OUT/$n.layout.txt"
}
# per-function Ir of one callgrind run: fn<TAB>ir
fn_ir() { # outfile
  callgrind_annotate --auto=no --threshold=100 --show=Ir --show-percs=no --inclusive=no "$1" 2>/dev/null \
    | awk '/^ *[0-9][0-9,]* +[^ ]/ { ir=$1; gsub(",","",ir); $1=""; sub(/^ +/,""); if ($0 ~ /PROGRAM TOTALS/) next
             sub(/ \[[^]]*\]$/,""); i=index($0,":"); if (i>0) $0=substr($0,i+1); gsub(/::h[0-9a-f]{16}/,"")
             if ($0 ~ /^0x[0-9a-f]+$/) $0="<unresolved address>"; a[$0]+=ir }
           END { for (f in a) printf "%s\t%d\n", f, a[f] }'
}
# writes $OUT/cg/$n.$arm.unstable (fn<TAB>min<TAB>max<TAB>spread): functions whose Ir differs between the
# runs of the same arm (work that depends on stack garbage, not on the code); they are excluded from the
# per-function +row gate and listed in summary.tsv. The witness total gate is unchanged.
unstable() { # arm name
  local arm=$1 n=$2 i
  for i in 1 2 3; do fn_ir "$OUT/cg/$n.$arm.$i.out" > "$OUT/cg/$n.$arm.$i.fn"; done
  awk -F'\t' '{ if (!($1 in mn) || $2<mn[$1]) mn[$1]=$2; if (!($1 in mx) || $2>mx[$1]) mx[$1]=$2; seen[$1]++ }
       END { for (f in mn) if (mx[f]!=mn[f] || seen[f]!=3) printf "%s\t%d\t%d\t%d\n", f, mn[f], mx[f], mx[f]-mn[f] }' \
    "$OUT/cg/$n.$arm.1.fn" "$OUT/cg/$n.$arm.2.fn" "$OUT/cg/$n.$arm.3.fn" | sort > "$OUT/cg/$n.$arm.unstable"
}
# prints "total" and writes $OUT/cg/$n.$arm.fn (fn<TAB>ir)
measure() { # arm name dir args  (runs $OUT/w/$n.$arm from the one path $OUT/w/$n)
  local arm=$1 n=$2 d=$3 a=$4 totals=() i
  cp "$OUT/w/$n.$arm" "$OUT/w/$n"
  for i in 1 2 3; do
    ( cd "$d"; ulimit -v 8000000; timeout 900 env -i "${ENVV[@]}" setarch x86_64 -R valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file="$OUT/cg/$n.$arm.$i.out" "$OUT/w/$n" $a > /dev/null 2> "$OUT/cg/$n.$arm.$i.log" )
    totals+=("$(grep -o 'Collected : [0-9]*' "$OUT/cg/$n.$arm.$i.log" | grep -o '[0-9]*$')")
  done
  unstable "$arm" "$n"
  if [ -z "${totals[0]}" ] || [ "${totals[0]}" != "${totals[1]}" ] || [ "${totals[0]}" != "${totals[2]}" ]; then
    echo "NONDETERMINISTIC $n $arm: ${totals[*]} (unstable functions: $(cut -f1,4 "$OUT/cg/$n.$arm.unstable" | tr '\t\n' ' ' | cut -c1-300))" >&2; return 1; fi
  cp "$OUT/cg/$n.$arm.1.fn" "$OUT/cg/$n.$arm.fn"
  echo "${totals[0]}"
}

printf "witness\ttotal_main\ttotal_head\tdelta\tfns_plus\n" > "$OUT/summary.tsv"; fail=0
for spec in "${W[@]}"; do
  IFS='|' read -r n d s a <<< "$spec"
  if [ -n "${IRDIFF_ONLY:-}" ] && [[ " $IRDIFF_ONLY " != *" $n "* ]]; then continue; fi
  if [ ! -f "$d/$s" ]; then echo "MISSING $n: no harness at $d/$s" | tee -a "$OUT/missing.txt"; printf "%s\tMISSING\tMISSING\t-\t-\n" "$n" >> "$OUT/summary.tsv"; continue; fi
  compile main "$MAIN" "$WS_MAIN" "$n" "$d" "$s" || { echo "COMPILE-FAIL $n main"; fail=1; continue; }
  compile head "$HEAD" "$WS_HEAD" "$n" "$d" "$s" || { echo "COMPILE-FAIL $n head"; fail=1; continue; }
  if [ "$LINKPAD" = 1 ]; then
    linkpad "$n" "$d" "$s" || { echo "LINKPAD-FAIL $n: layouts differ (see $OUT/$n.layout.txt, $OUT/pads.tsv)"; fail=1; continue; }
  fi
  tm=$(measure main "$n" "$d" "$a") || { fail=1; continue; }
  th=$(measure head "$n" "$d" "$a") || { fail=1; continue; }
  rm -f "$OUT/w/$n" "$OUT/w/$n.main" "$OUT/w/$n.head"
  awk -F'\t' 'NR==FNR { m[$1]=$2; next } { h[$1]=$2 } END { for (f in m) u[f]; for (f in h) u[f]; for (f in u) printf "%s\t%d\t%d\t%d\n", f, m[f]+0, h[f]+0, h[f]-m[f] }' \
    "$OUT/cg/$n.main.fn" "$OUT/cg/$n.head.fn" | awk -F'\t' '$4!=0' | sort -t$'\t' -k4,4nr > "$OUT/$n.tsv"
  grep -v '^#' "$ALLOW" 2>/dev/null | sed '/^$/d' | sort -u > "$OUT/tmp/allow.$n"
  plus=$(awk -F'\t' '$4>0' "$OUT/$n.tsv" | wc -l)
  cat "$OUT/cg/$n.main.unstable" "$OUT/cg/$n.head.unstable" | cut -f1 | sort -u > "$OUT/tmp/unst.$n"
  # a +row is checked unless it is allowed or unstable; FILENAME (not NR==FNR) so an empty list does not pass vacuously
  bad=$(awk -F'\t' 'FILENAME==ARGV[1] || FILENAME==ARGV[2] { ok[$0]; next } $4>0 && !($1 in ok)' "$OUT/tmp/allow.$n" "$OUT/tmp/unst.$n" "$OUT/$n.tsv" | wc -l)
  for a in main head; do awk -F'\t' -v w="$n" -v a="$a" '{ printf "unstable\t%s\t%s\t%s\tspread=%d\n", w, a, $1, $4 }' "$OUT/cg/$n.$a.unstable" >> "$OUT/unstable.tsv"; done
  printf "%s\t%s\t%s\t%d\t%d\n" "$n" "$tm" "$th" $((th-tm)) "$plus" >> "$OUT/summary.tsv"
  [ "$th" -gt "$tm" ] && { echo "TOTAL ABOVE MAIN: $n (+$((th-tm)))"; fail=1; }
  [ "$bad" -gt 0 ] && { echo "UNEXPLAINED +Ir in $n: $bad functions (see $OUT/$n.tsv)"; fail=1; }
done
[ -f "$OUT/unstable.tsv" ] && cat "$OUT/unstable.tsv" >> "$OUT/summary.tsv"
[ -f "$OUT/pads.tsv" ] && sed 's/^/linkpad\t/' "$OUT/pads.tsv" >> "$OUT/summary.tsv"
cat "$OUT/summary.tsv"
echo "IRDIFF-DONE fail=$fail"
exit $fail
