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
# Measurement controls are the accepted JST recipe (nightly/CW/STARTUP_RULE_2026-10-28.md): a minimal
# environment, the fixed-inputs preload (clock, entropy, /proc/self/maps normalization), ASLR off,
# --cache-sim=no --branch-sim=no. IRDIFF_PRELOAD overrides the preload path.
# IRDIFF_ONLY="hello commander_help" limits the run to those witnesses.
# Each perry binary sits next to its runtime libs (PERRY_RUNTIME_DIR = dirname of the resolved
# binary). Workspace roots: IRDIFF_WS_MAIN / IRDIFF_WS_HEAD (defaults /root/A1S0/main-src and
# /root/A1S0/src). Witness roots: IRDIFF_SCORE (/root/hwp-move/score), IRDIFF_SAMPLES (/root/STU/samples),
# IRDIFF_NSB (/root/NSB/jsc).
set -u
full=0; pos=()
for a in "$@"; do [ "$a" = --full ] && full=1 || pos+=("$a"); done
MAIN=$(readlink -f "${pos[0]:?perry-main}"); HEAD=$(readlink -f "${pos[1]:?perry-head}"); OUT=${pos[2]:?outdir}
ALLOW=${pos[3]:-/dev/null}
SC=${IRDIFF_SCORE:-/root/hwp-move/score}; SAMPLES=${IRDIFF_SAMPLES:-/root/STU/samples}; NSB=${IRDIFF_NSB:-/root/NSB/jsc}
WS_MAIN=${IRDIFF_WS_MAIN:-/root/A1S0/main-src}; WS_HEAD=${IRDIFF_WS_HEAD:-/root/A1S0/src}
mkdir -p "$OUT/w" "$OUT/cg" "$OUT/tmp"; OUT=$(readlink -f "$OUT")
PRELOAD=${IRDIFF_PRELOAD:-/root/JST/jst-cg/measurement/jst-fixed-inputs.so}
[ -f "$PRELOAD" ] || { echo "missing measurement preload $PRELOAD" >&2; exit 2; }
# minimal environment of both the compile and the run (same for both arms)
ENVV=(PATH=/usr/lib/llvm-22/bin:/root/.cargo/bin:/usr/bin:/bin HOME="$HOME" LANG=C LC_ALL=C TZ=UTC LD_PRELOAD="$PRELOAD" PERRY_ALLOW_PERRY_FEATURES=1 TMPDIR="$OUT/tmp" NO_COLOR=1 PKG_BENCH_N=0 PKG_BENCH_WARM=0)

# name|dir|source|args  (args are run as given; dir is the cwd of both compile and run)
W=("hello|$SAMPLES|01-hello/main.ts|Perry"
   "commander_help|$SC/p/pk|commander/parse_argv.ts|--help"
   "fastify_inject|$SC/p/pk|fastify/inject.ts|"
   "awfy_richards|$SC/p/awfy|harness.js|Richards 1 0"
   "binary_trees|$NSB|benches/23-binary-trees.ts|")
if [ $full = 1 ]; then
  W+=("tsc_tssrc|$SC/p/tssrc/work|tssrc.ts|3"
      "claude_code_newest|${IRDIFF_CLAUDE:-/root/secret-tests/claude-code-newest}|${IRDIFF_CLAUDE_SRC:-unbun-out/main.js}|--version"
      "opencode_target|${IRDIFF_OPENCODE:-/root/secret-tests/opencode-target}|${IRDIFF_OPENCODE_SRC:-split/main.js}|--version"
      "nextjs_hello|${IRDIFF_NEXT:-/root/secret-tests/nextjs-hello}|${IRDIFF_NEXT_SRC:-main.ts}|")
fi

compile() { # arm perry ws name dir src
  local arm=$1 perry=$2 ws=$3 n=$4 d=$5 s=$6
  ( cd "$d" && env -i "${ENVV[@]}" PERRY_RUNTIME_DIR=$(dirname "$perry") PERRY_WORKSPACE_ROOT=$ws PERRY_TARGET_CPU=x86-64-v3 PERRY_KEEP_SYMBOLS=1 \
      "$perry" compile --cache-dir "$OUT/tmp/cache" "$s" -o "$OUT/w/$n" > "$OUT/cg/$n.$arm.compile.log" 2>&1 )
  local rc=$?; rm -rf "$OUT/tmp/cache"; return $rc
}
# prints "total" and writes $OUT/cg/$n.$arm.fn (fn<TAB>ir)
measure() { # arm name dir args
  local arm=$1 n=$2 d=$3 a=$4 totals=() i
  for i in 1 2 3; do
    ( cd "$d"; ulimit -v 8000000; timeout 900 env -i "${ENVV[@]}" setarch x86_64 -R valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file="$OUT/cg/$n.$arm.$i.out" "$OUT/w/$n" $a > /dev/null 2> "$OUT/cg/$n.$arm.$i.log" )
    totals+=("$(grep -o 'Collected : [0-9]*' "$OUT/cg/$n.$arm.$i.log" | grep -o '[0-9]*$')")
  done
  if [ -z "${totals[0]}" ] || [ "${totals[0]}" != "${totals[1]}" ] || [ "${totals[0]}" != "${totals[2]}" ]; then
    echo "NONDETERMINISTIC $n $arm: ${totals[*]}" >&2; return 1; fi
  callgrind_annotate --auto=no --threshold=100 --show=Ir --show-percs=no --inclusive=no "$OUT/cg/$n.$arm.1.out" 2>/dev/null \
    | awk '/^ *[0-9][0-9,]* +[^ ]/ { ir=$1; gsub(",","",ir); $1=""; sub(/^ +/,""); if ($0 ~ /PROGRAM TOTALS/) next
             sub(/ \[[^]]*\]$/,""); i=index($0,":"); if (i>0) $0=substr($0,i+1); gsub(/::h[0-9a-f]{16}/,"")
             if ($0 ~ /^0x[0-9a-f]+$/) $0="<unresolved address>"; a[$0]+=ir }
           END { for (f in a) printf "%s\t%d\n", f, a[f] }' > "$OUT/cg/$n.$arm.fn"
  echo "${totals[0]}"
}

printf "witness\ttotal_main\ttotal_head\tdelta\tfns_plus\n" > "$OUT/summary.tsv"; fail=0
for spec in "${W[@]}"; do
  IFS='|' read -r n d s a <<< "$spec"
  if [ -n "${IRDIFF_ONLY:-}" ] && [[ " $IRDIFF_ONLY " != *" $n "* ]]; then continue; fi
  if [ ! -f "$d/$s" ]; then echo "MISSING $n: no harness at $d/$s" | tee -a "$OUT/missing.txt"; printf "%s\tMISSING\tMISSING\t-\t-\n" "$n" >> "$OUT/summary.tsv"; continue; fi
  compile main "$MAIN" "$WS_MAIN" "$n" "$d" "$s" || { echo "COMPILE-FAIL $n main"; fail=1; continue; }
  tm=$(measure main "$n" "$d" "$a") || { fail=1; continue; }
  compile head "$HEAD" "$WS_HEAD" "$n" "$d" "$s" || { echo "COMPILE-FAIL $n head"; fail=1; continue; }
  th=$(measure head "$n" "$d" "$a") || { fail=1; continue; }
  rm -f "$OUT/w/$n"
  awk -F'\t' 'NR==FNR { m[$1]=$2; next } { h[$1]=$2 } END { for (f in m) u[f]; for (f in h) u[f]; for (f in u) printf "%s\t%d\t%d\t%d\n", f, m[f]+0, h[f]+0, h[f]-m[f] }' \
    "$OUT/cg/$n.main.fn" "$OUT/cg/$n.head.fn" | awk -F'\t' '$4!=0' | sort -t$'\t' -k4,4nr > "$OUT/$n.tsv"
  grep -v '^#' "$ALLOW" 2>/dev/null | sed '/^$/d' | sort -u > "$OUT/tmp/allow.$n"
  plus=$(awk -F'\t' '$4>0' "$OUT/$n.tsv" | wc -l)
  bad=$(awk -F'\t' 'NR==FNR { ok[$0]; next } $4>0 && !($1 in ok)' "$OUT/tmp/allow.$n" "$OUT/$n.tsv" | wc -l)
  printf "%s\t%s\t%s\t%d\t%d\n" "$n" "$tm" "$th" $((th-tm)) "$plus" >> "$OUT/summary.tsv"
  [ "$th" -gt "$tm" ] && { echo "TOTAL ABOVE MAIN: $n (+$((th-tm)))"; fail=1; }
  [ "$bad" -gt 0 ] && { echo "UNEXPLAINED +Ir in $n: $bad functions (see $OUT/$n.tsv)"; fail=1; }
done
cat "$OUT/summary.tsv"
echo "IRDIFF-DONE fail=$fail"
exit $fail
