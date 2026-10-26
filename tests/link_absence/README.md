# Runtime link absence (HWS)

Build the compiler and coherent archives, generate the GC sidecars, then run:

```sh
cargo build-perry --release
scripts/gc_call_effects/sidecars.sh target/release
PERRY_RUNTIME_DIR="$PWD/target/release" python3 scripts/link_absence.py \
  --perry target/release/perry --output target/link-absence
```

Use the repository's `.node-version` and LLVM 22 (`LLVM_SYS_221_PREFIX` locates
its tools). ELF diagnostics also require `ld.lld` from that directory. The
required full-tier `binary-size` matrix runs this command on Ubuntu and macOS;
its artifacts include compile logs, Node/native output and `report.json`.

`without_math.ts` uses stdout to prove Math absence independently of the
compiler's conservative `log` property detection. `hello.ts` reads argv to exercise the normal runtime rather than the tiny
program path. `without_regex.ts` checks literal replacement, callbacks, custom
symbol methods and UTF-16 splitting with no regex install. Deserializers that can
receive a RegExp from another program (V8, IPC and workers) select that install. Every runtime hook in `stdlib_installs.rs::RUNTIME_INSTALLS` must
have a positive fixture. Each fixture declares its hook and symbol prefixes;
explicit unhooked fixtures say `none (no hook yet)`. Prefixes match demangled
text symbols, including C entry points. Text bytes are section size; attributed
bytes are symbol address spans in that section, deduplicated at aliased
addresses. Rules can overlap, so subsystem bytes must not be summed.

The gate keeps symbols, disables compiler caches, verifies selected install
entry points, compares every positive's stdout and exit status with Node, and
requires its rule to find a linked symbol. Missing tools, failed compilation,
missing install evidence, failed Node execution and empty positive matches
fail the test. Native warning text on stderr is recorded separately; stdout is the deterministic parity oracle.

Current bypasses are explicit symbol-name exceptions owned by **HWS2**, per
platform under `known-failing/`. New matching symbols fail even inside a
subsystem with existing exceptions. When a subsystem disappears entirely,
the stale exception fails: remove it in the fix to lock in absence. A regression
then fails. Rust disambiguation hashes are excluded from exception names.
An unchanged whitelisted symbol set cannot reveal an additional retaining edge;
these exceptions are debt to remove, not evidence that existing leaks are safe.

A failure relinks the negative witness with the platform's `why_live` option
for the subsystem's largest symbol and prints the real linker chain. Full
build logs stay in the output directory. `--record-known-failures` is only for
reviewed baseline measurement; CI never passes it. `--negative-only` is a diagnostic
and sabotage shortcut; CI always executes all positive witnesses.

The explicit stdlib witnesses `bundled_web_streams.ts` and `brotli_streams.ts`
exercise FileHandle streams (including native property bags) and Brotli output.
`fs_promises_without_streams.ts` reads a file without a stream API. Its required
absence checks also reject a wrongly selected streams installer, so compiler
over-selection cannot silently exempt that negative from the symbol check.
A `HWS companion` declaration adds an independent positive for an existing
absence rule. Its prefixes, including its hook, must all be covered by that rule. This keeps the FileHandle API-selection test independent of the
Brotli constructor test while requiring both engines absent in the file read.

Native-module installers are selected in namespace-creation code rather than the runtime feature object. The gate reads their undefined symbols from the compiler-generated objects in the actual link invocation. References originating inside an archive cannot exempt a negative; native positives must both name their installer in a program object and exercise its API. `without_node_stream.ts` checks Array subclasses and ordinary objects with no stream install, while the stream and events-only positives exercise both owning installers.
