# Unpack CPU benchmark

This fixture runs upm's unpack worker without editing its source. It uses the
preserved 22-tarball corpus, including the 8,075,816-byte
`@rolldown/binding-linux-x64-gnu@1.2.12` tarball at index 1. `corpus.json` pins
the compressed bytes with SHA-256 and records both compressed and raw sizes.
The tarballs and upstream sources are external, read-only inputs.

Prepare the driver next to byte-for-byte copies of the upstream modules:

```sh
python3 prepare_micro.py /root/lanes/upm-prof/upm/src \
  /root/lanes/perry-tarloop/micro/corpus /tmp/unpackcpu-fixture
PERRY_RUNTIME_DIR="$CARGO_TARGET_DIR/release" \
  "$CARGO_TARGET_DIR/release/perry" compile /tmp/unpackcpu-fixture/micro.ts \
  --no-cache -o /tmp/unpackcpu-micro
```

Compile both source revisions with the default auto-optimization. Rebuild
`perry`, `perry-runtime-static` and `perry-stdlib-static` before compiling a
program after changing crate sources. Preserve symbols with
`PERRY_KEEP_SYMBOLS=1`. Do not set `PERRY_NO_AUTO_OPTIMIZE`.

Arguments are `MODE SELECTION ROUNDS STORE`. `SELECTION` is `large` or `all`.
Modes are `worker`, `worker-stream`, `worker-noop`, `noop`, `hash`, `sha256`,
`sha1`, `inflate` and `parse`. Both worker modes use owning 64 KiB input
blocks; the streaming mode sends them as separate messages. Supplying
subarray views backed by a complete tarball makes structured clone copy
that backing store repeatedly in Node and is not a representative input.
Pass a disposable store path explicitly to either worker mode.

```sh
/tmp/unpackcpu-micro worker-stream all 1 /tmp/unpackcpu-store
node --experimental-strip-types /tmp/unpackcpu-fixture/micro.ts \
  worker-stream all 1 /tmp/unpackcpu-node-store
```

The public worker index, payload byte count and entry count must match Node.
Compare every written file's mode and SHA-256 outside the measured interval.
The worker-ready noop separates startup from processing; subtracting it is
an estimate because allocation and collection are not strictly additive.

The backend regression witness uses the actual Perry crypto API. On the qb6
AVX2/BMI1/BMI2 host it runs 16 passes over the pinned corpus, checks all
digests against Node and requires at most 6.5 billion user instructions:

```sh
python3 backend_budget.py /tmp/unpackcpu-micro \
  /tmp/unpackcpu-fixture/micro.ts --node /path/to/node24 --output /tmp/witness
OPENSSL_ia32cap=':~0x128' python3 backend_budget.py /tmp/unpackcpu-micro \
  /tmp/unpackcpu-fixture/micro.ts --node /path/to/node24 --output /tmp/masked
```

The unchanged-main binary must fail the budget and the accelerated binary
must pass it. Masking AVX2/BMI1/BMI2 must preserve digests and fail the budget.
This performance test needs Linux perf access and permission to disable
ASLR. It is a CPU-qualified fixture witness, not a general wall-time test.

Use five interleaved runs on CPUs 0-55 for `instructions:u`, with
`setarch -R`. Measure cycles and wall time only on CPUs 56-63 while holding
`/root/MEASURE.lock`; cold upm uses seven runs. Record user CPU and max RSS
with `wait4`, and full collection counts in separate diagnostic replays.
Use a per-process `PR_SET_THP_DISABLE` control for RSS differences near 2 MiB.

Perf profiles must retain both call chains and sampled functions. Assembly
samples can have empty call chains: `perf script -G` still reports their
sampled function. Join that stream with the regular stack dump by sample
order and assert the thread IDs and periods agree. Weight stages by sampled
periods; keep GC/pacing, regex, byte handling, object work and dispatch visible.

The measured source provenance, native backend probes, raw interleaved rows,
medians/spread, test results and remaining questions accompany `REPORT.md`.
