Private members, arguments and typed-array parameter access (#10501,
#11791, #10509, #10515, #10516).

`constructs.ts` supplies the common definitions. `measure.py` generates one
branch-free TypeScript kernel and JavaScript twin per construct in the lane's
`micros` directory. No runtime selector runs inside a measured kernel.

On Linux, with separately built `main/{src,target}` and `head/{src,target}`:

```
python3 micros/measure.py /root/codex-lanes/cx-privargs rank
python3 micros/measure.py /root/codex-lanes/cx-privargs compile
python3 micros/measure.py /root/codex-lanes/cx-privargs compare
```

Copy the template and script into `micros/`, the shared class micros into
`class-micros/`, and the real-program drivers and their installed dependencies
into `drivers/`. The script does not build Rust or install packages.

Measurements use instructions:u, setarch -R, inherited THP-disable, five
interleaved trials, and `(N200000 - N50000) / 150000` after a fixed warmup.
Node uses acknowledged perf control FIFOs around the warmed-up kernel,
explicit TurboFan optimization and disabled function inlining. Node startup
and compilation are outside its counter window. Native whole-process slopes
cancel fixed startup/warmup cost. RSS is whole-process maximum, in KiB.
The real-program A/B also records emitted .text and separate GC traces.

`--arms main args` verifies arguments birth independently; `--arms args head`
verifies method entry home removal. Each arm has its own source and target.
Use `--label arguments` / `--label home` to retain independent A/B summaries.
`verify.py` compares the focused `.ts` and sloppy `.cts` witnesses against
Node and the reference arm. `run_controls.py` checks that copied binaries
with arguments birth or fresh-evaluation home calls removed lose parity.

Current main already stores private brands in shapes and directly calls
eligible private methods after the brand guard. Typed-array admission reads
the receiver's byte-cell header; unrelated aliasing views do not disable it.
The lane therefore measures these existing mechanisms rather than adding
another brand cache or typed-array admission path. Arguments snapshots remain
managed Arrays: native frame snapshots need precise statepoint support for
their mutable slots before packing can safely be removed.
