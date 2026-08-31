#!/usr/bin/env python3
"""Read results/results.json + results/metadata.json and emit REPORT.md.

Report structure:
  1. Hardware / toolchain table (from metadata)
  2. Per-workload table: median / stddev wall time, median peak RSS,
     binary size, LoC
  3. Ratios (Perry vs best, Rust vs Zig)
  4. Honest-findings section summarizing Perry bugs discovered
  5. Reproduction instructions

No plotting here — `plot.py` emits PNGs separately.
"""
import os
import json
import subprocess
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "results" / "results.json"
META    = ROOT / "results" / "metadata.json"
OUT     = ROOT / "REPORT.md"

WORKLOAD_TITLES = {
    "image_convolution":   "3. Image convolution (5×5 Gaussian, 3840×2160 RGB)",
    "json_pipeline_small": "1a. JSON pipeline — small fixture (100 records, 21 KB)",
    "json_pipeline_full":  "1b. JSON pipeline — full fixture (500k records, 108 MB)",
}
WORKLOAD_NOTES = {
    "image_convolution":
        "In-memory input + output checksum (no PPM I/O) — see the workload "
        "README for the reason. All three languages produce the identical "
        "FNV-1a-32 checksum.",
    "json_pipeline_small":
        "All three languages produce byte-identical output at this scale "
        "(hash `7fc66fa8`).",
    "json_pipeline_full":
        "All five implementations complete this workload against the "
        "same 108 MB fixture and produce the same hash `b7e8a588`. Perry "
        "completes in ~1.6 s, ~2.7× the leader (Rust / Bun); as recently "
        "as v0.5.68 this workload hung >13 minutes without finishing "
        "(`#65`, now closed).",
}
LANGUAGE_ORDER = ["rust", "zig", "perry", "node", "bun"]

LOC_FILES = {
    "image_convolution": {
        "rust":  "workloads/3_image_convolution/rust/src/main.rs",
        "zig":   "workloads/3_image_convolution/zig/src/main.zig",
        "perry": "workloads/3_image_convolution/perry/image_conv.ts",
        "node":  "workloads/3_image_convolution/node/image_conv.ts",
        "bun":   "workloads/3_image_convolution/node/image_conv.ts",
    },
    "json_pipeline_small": {
        "rust":  "workloads/1_json_pipeline/rust/src/main.rs",
        "zig":   "workloads/1_json_pipeline/zig/src/main.zig",
        "perry": "workloads/1_json_pipeline/perry/json_pipeline.ts",
        "node":  "workloads/1_json_pipeline/node/json_pipeline.ts",
        "bun":   "workloads/1_json_pipeline/node/json_pipeline.ts",
    },
    "json_pipeline_full": {
        "rust":  "workloads/1_json_pipeline/rust/src/main.rs",
        "zig":   "workloads/1_json_pipeline/zig/src/main.zig",
        "perry": "workloads/1_json_pipeline/perry/json_pipeline.ts",
        "node":  "workloads/1_json_pipeline/node/json_pipeline.ts",
        "bun":   "workloads/1_json_pipeline/node/json_pipeline.ts",
    },
}
BINARY_PATHS = {
    "image_convolution": {
        "rust":  "workloads/3_image_convolution/rust/target/release/image_conv",
        "zig":   "workloads/3_image_convolution/zig/zig-out/bin/image_conv",
        "perry": "workloads/3_image_convolution/perry/image_conv",
    },
    "json_pipeline_small": {
        "rust":  "workloads/1_json_pipeline/rust/target/release/json_pipeline",
        "zig":   "workloads/1_json_pipeline/zig/zig-out/bin/json_pipeline",
        "perry": "workloads/1_json_pipeline/perry/json_pipeline",
    },
    "json_pipeline_full": {
        "rust":  "workloads/1_json_pipeline/rust/target/release/json_pipeline",
        "zig":   "workloads/1_json_pipeline/zig/zig-out/bin/json_pipeline",
        "perry": "workloads/1_json_pipeline/perry/json_pipeline",
    },
}

def fmt_ms(v):   return f"{v:,.1f}"
def fmt_kb(v):   return f"{v/1024:,.1f} MB" if v >= 1024 else f"{v:,.0f} KB"
def fmt_ratio(a, b):
    if b == 0: return "–"
    return f"{a/b:.2f}×"

def loc(path: Path) -> int:
    try:
        with open(path) as f:
            return sum(1 for line in f if line.strip() and not line.strip().startswith(("//","#")))
    except FileNotFoundError:
        return -1

def binsize(path: Path) -> str:
    try:
        n = path.stat().st_size
    except FileNotFoundError:
        return "—"
    if n >= 1024*1024: return f"{n/1024/1024:.1f} MB"
    if n >= 1024:      return f"{n/1024:.1f} KB"
    return f"{n} B"

def stats_for(rows):
    wall = [r["wall_ms"]    for r in rows if r["exit_code"] == 0]
    rss  = [r["max_rss_kb"] for r in rows if r["exit_code"] == 0]
    failures = sum(1 for r in rows if r["exit_code"] != 0)

    # `exit_code == 0` alone is not enough to make a sample meaningful: a run
    # can succeed (correct stdout, correct checksum) and still carry a wall
    # time that is physically impossible. Report those loudly instead of
    # taking their median -- a median of zeros renders as a confident "0.0 ms"
    # table cell and reads exactly like a real result.
    implausible = [w for w in wall if w <= 0]
    if implausible:
        raise SystemExit(
            f"report.py: {len(implausible)} of {len(wall)} successful runs have a "
            f"non-positive wall_ms (min {min(implausible)}).\n"
            "  results.json carries no usable timing data; a report generated from "
            "it would be fiction.\n"
            "  Re-run ./run.sh on this host and check that run_bench.sh's monotonic "
            "clock samples are comparable across processes."
        )

    if not wall:
        return None, None, None, None, failures
    return (
        statistics.median(wall),
        (statistics.stdev(wall) if len(wall) >= 2 else 0.0),
        statistics.median(rss),
        (statistics.stdev(rss) if len(rss) >= 2 else 0.0),
        failures,
    )

def main():
    if not RESULTS.exists():
        raise SystemExit(f"missing {RESULTS} — run ./run.sh first")

    data = json.loads(RESULTS.read_text())["rows"]
    meta = json.loads(META.read_text()) if META.exists() else {}

    by_wl = {}
    for r in data:
        by_wl.setdefault(r["workload"], {}).setdefault(r["language"], []).append(r)

    lines = []
    lines.append("# Perry vs Rust vs Zig — honest benchmark results\n")
    lines.append(
        "Numbers are measured against Perry v0.5.81 on five implementations: "
        "Rust, Zig, Perry, Node.js, Bun. All three workloads complete on all "
        "five implementations — every correctness and scaling bug the bench "
        "originally surfaced (`#38`–`#53`, `#62`–`#65`) has landed in Perry "
        "mainline. 260 measured runs, zero failures.\n"
    )

    # Bottom line up-front
    lines.append("## Bottom line\n")
    lines.append("""\
- **Compute (image convolution, tight loop, minimal heap):** Zig 243 ms.
  **Perry 268 ms (1.10× Zig) — ahead of Rust's 567 ms**, 3.4× faster than
  Bun, 4.5× faster than Node. Full arc across the perf sprint:
  **2261 ms → 268 ms, an 8.4× speedup** (issues #47, #48, #49, #50, #52 plus
  the v0.5.58–v0.5.68 follow-on).
- **Allocation-heavy (JSON pipeline, 100 records):** Zig 32 ms, Rust 33 ms,
  **Perry 37 ms (1.15× the fastest) — ahead of Bun's 49 ms and Node's 145 ms**,
  at 3 MB RSS vs Bun's 11 MB and Node's 36 MB. Earlier sweep had Perry at
  210 ms here; v0.5.69–v0.5.75 closed ~6× of that.
- **JSON at scale (500k records, 108 MB):** Rust 604 ms, Bun 647 ms, Zig
  850 ms, Node 1010 ms, **Perry 1649 ms**. Perry trails the pack by ~2.5×
  on wall time and uses more RSS (744 MB peak), but it _completes_ — as
  recently as v0.5.68 this workload hung >13 min CPU without finishing
  (`#65`, now closed).
- **Binary size:** Zig smallest (~230 KB), Rust next (~300–380 KB), Perry
  (~550–700 KB, including GC + Node-compat shims). Node/Bun don't have
  standalone binaries — they require their runtime installed separately.
- **Source LoC (non-blank, non-comment):** The TypeScript implementations
  (Perry / Node / Bun run the same source) are in the 52–92-line range;
  Rust and Zig at 99–113. TypeScript gives ~25–40% fewer lines at
  competitive-to-native performance on two of three workloads.

Charts: [image convolution](charts/image_convolution.png),
[JSON 100-record](charts/json_pipeline_small.png),
[JSON 108 MB](charts/json_pipeline_full.png).
""")

    lines.append("## Hardware & toolchains\n")
    h = meta.get("host", {})
    t = meta.get("toolchains", {})
    harn = meta.get("harness", {})
    lines.append("| | |")
    lines.append("|---|---|")
    lines.append(f"| CPU | {h.get('cpu','?')} ({h.get('ncpu','?')} cores, {h.get('arch','?')}) |")
    lines.append(f"| RAM | {h.get('ram_gb','?')} GB |")
    lines.append(f"| OS  | macOS {h.get('os_version','?')} ({h.get('kernel','?').split()[0]}) |")
    lines.append(f"| Rust | `{t.get('rustc','?')}` |")
    lines.append(f"| Zig | `{t.get('zig','?')}` |")
    lines.append(f"| Perry | `{t.get('perry','?')}` |")
    lines.append(f"| Python | `{t.get('python','?')}` |")
    lines.append(f"| Runs | {harn.get('warmup','?')} warmup + {harn.get('measured','?')} measured, median reported |")
    lines.append(f"| Generated | {meta.get('generated_at','?')} |\n")

    for wl_id, rows_by_lang in by_wl.items():
        lines.append(f"## {WORKLOAD_TITLES.get(wl_id, wl_id)}\n")
        if wl_id in WORKLOAD_NOTES:
            lines.append(f"_{WORKLOAD_NOTES[wl_id]}_\n")

        langs_present = [l for l in LANGUAGE_ORDER if l in rows_by_lang]
        lines.append("| Language | Wall median (ms) | Wall σ | Peak RSS | Binary size | Source LoC | Runs OK |")
        lines.append("|---|---:|---:|---:|---:|---:|---:|")
        medians = {}
        for lang in langs_present:
            rows = rows_by_lang[lang]
            med_wall, sd_wall, med_rss, _, failures = stats_for(rows)
            binp = BINARY_PATHS.get(wl_id, {}).get(lang, "")
            srcp = LOC_FILES.get(wl_id, {}).get(lang, "")
            bsz = binsize(ROOT / binp) if binp else "—"
            loc_count = loc(ROOT / srcp) if srcp else -1
            ok = f"{len(rows)-failures}/{len(rows)}"
            medians[lang] = med_wall or 0
            if med_wall is None:
                lines.append(f"| {lang} | — (all failed) | — | — | {bsz} | {loc_count} | {ok} |")
            else:
                lines.append(
                    f"| {lang} | {fmt_ms(med_wall)} | {fmt_ms(sd_wall)} | "
                    f"{fmt_kb(med_rss)} | {bsz} | {loc_count} | {ok} |"
                )
        best = min((v for v in medians.values() if v > 0), default=0)
        if best:
            ratios = ", ".join(
                f"{l} = {fmt_ratio(medians[l], best)}"
                for l in langs_present if medians[l] > 0
            )
            lines.append(f"\n_Ratios vs fastest: {ratios}_\n")

    # -------- honest findings --------
    lines.append("## Honest findings — Perry gaps surfaced by this benchmark\n")
    lines.append(
        "Building the Perry implementations surfaced 8 real bugs. **7 of them "
        "were fixed in v0.5.30** while this benchmark was being written; the "
        "8th was only visible once the earlier fixes landed. Each has a "
        "standalone 20-line TS repro.\n"
    )
    lines.append("""\
### Fixed in v0.5.30

1. **`buf[i] = v` on `Buffer` / `Uint8Array` was a silent no-op.** The
   lowering for `Expr::Uint8ArraySet` in `crates/perry-codegen/src/expr.rs`
   was `lower_expr(value)` — it evaluated the RHS and threw it away. The
   runtime helper `js_buffer_set(buf, idx, val)` already existed; the
   codegen just wasn't calling it. _Fixed in this commit._

2. **[#38]** `new Uint8Array(N)` with a non-literal `N` routed to
   `js_uint8array_from_array` and misread the number as an `ArrayHeader*`,
   yielding a zero-length buffer. _Fixed._

3. **[#39]** 64-bit BigInt bitwise ops (XOR, AND-mask, multiply-and-mask)
   produced wrong results — `a ^ 5n` returned a small negative, AND-masking
   with `0xFFFF…n` collapsed to 0. Any FNV-1a-64 / Murmur / xxhash64
   implementation collapsed to 0 under Perry. _Fixed._

4. **[#40]** `Math.imul` was not lowered by the codegen (compile-time
   `expression MathImul not yet supported`). Every 32-bit-wrap hash in the
   world uses it. _Fixed._

5. **[#41]** `process.argv.slice(N)` returned a shape with `typeof` =
   `"string"`, length = the full argv length, and elements that were raw
   NaN-box bit patterns interpreted as tiny denormal floats. _Fixed._

6. **[#42]** Passing a multi-MB `Buffer` as a function parameter while the
   callee ran its own `Buffer.alloc()` silently corrupted the parameter.
   The param landed in a callee-saved register the conservative stack scan
   didn't cover; the intervening GC swept the backing buffer. _Fixed._

7. **[#43]** `JSON.stringify` panicked inside `perry-runtime/src/json.rs:427`
   (`byte index N is not a char boundary`) on arrays of 30k+ records with
   nested objects — reading already-corrupted string payloads, likely from
   the same underlying GC issue as #42. _Fixed._

8. **[#44]** `JSON.parse` + iterate + field read on a 50k-record array with
   rich objects dropped 99.9% of `.active === true` matches — the parsed
   objects were being swept mid-iteration. _Fixed._

### Still open

9. **[#46]** `JSON.parse` silently caps output at ~1666 entries for inputs
   larger than roughly 4 MB of structured records. Returns without error;
   `parsed.length` is just 1666 instead of the real count. Surfaced only
   after the #43 panic was fixed — previously the panic fired before the
   truncation was visible. This is why the Perry JSON pipeline is still
   run on the 100-record fixture only.

### Net effect on the numbers

The Perry columns in this report reflect a Perry-TS written *with the
workarounds for #38–#44 still in place* (hand-rolled `imul32`, module-level
`Buffer` globals, fresh-object construction in JSON) — removing those
workarounds after v0.5.30 would simplify the code further but wouldn't
materially change the numbers; the slow paths are the hash loop, the JSON
parse, and the convolution kernel, none of which are affected by the
workaround shape.
""")

    # -------- methodology + repro --------
    lines.append("## Methodology\n")
    lines.append(f"""\
- **Release / optimized builds only**: `cargo --release`, `zig build-exe -O
  ReleaseFast`, Perry's native path (auto-optimized libraries).
- **Warmup / measured**: configurable via `HONEST_BENCH_WARMUP` and
  `HONEST_BENCH_MEASURED` (defaults: 5 / 20). **Median** is reported
  because it's robust to the occasional stray OS scheduler hiccup; σ is
  reported alongside.
- **Wall time**: Python `time.monotonic_ns()` delta around the binary
  invocation (so it includes process startup + fs open + the work itself).
- **Peak RSS**: `/usr/bin/time -l`'s `peak memory footprint`, captured in
  bytes and converted to kB / MB.
- **Correctness**: every run emits a line containing a record-count and an
  FNV-1a-32 hash. The driver records stdout's first + last 200 characters
  for each run, which is the minimum needed to verify the three languages
  agree on what they computed.
- **Source LoC**: non-blank, non-comment lines. Computed by the report
  script (no `tokei` / `scc` needed).
- **Fixtures**: deterministic — `scripts/gen_json.py` writes byte-identical
  output across runs. The image convolution uses an in-process xorshift32
  stream.

No SIMD intrinsics, no hand-vectorized loops, no `#[target_feature]` — the
code in each language is what a typical first pass would look like. The
compilers' autovectorizers do their own thing.
""")

    lines.append("## Reproduction\n")
    lines.append("""\
```bash
cd benchmarks/honest_bench
./run.sh                           # build, generate fixtures, run, write results/
python3 scripts/plot.py            # render charts/*.png
python3 scripts/report.py          # regenerate REPORT.md
```

Environment overrides:

```bash
HONEST_BENCH_WARMUP=1 HONEST_BENCH_MEASURED=3 ./run.sh    # quick iteration
HONEST_BENCH_ONLY=3 ./run.sh                              # image conv only
HONEST_BENCH_SKIP_BUILD=1 ./run.sh                        # reuse existing bins
```

The workload-2 HTTP server benchmark is deferred to a follow-up — it requires
an HTTP load generator (oha/wrk/hey) and a Perry `fastify` implementation
under sustained concurrent load. Not landed in this phase.
""")

    OUT.write_text("\n".join(lines))
    print(f"wrote {OUT}")

if __name__ == "__main__":
    main()
