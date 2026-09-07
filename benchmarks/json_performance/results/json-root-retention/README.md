# JSON lifetime root investigation

The earlier 25-versus-35 collection discrepancy is reproducible in unchanged
binaries, and its first material retention difference is now identified.
**The checkpoint accidentally retains more garbage and therefore collects less.**
The rejected shared-admission serializer does not introduce extra GC-policy work;
its changed native frames alter which stale pointers the conservative scan finds.

These are local LLDB diagnostics, not CPU or RSS benchmarks. Debugger stops and
PC interventions invalidate timing measurements. No production scan was removed,
no host debugger permissions were changed, and neither binary was patched on disk.
The benchmark Mac refused debugger launch over SSH; local macOS permitted it.
The unchanged local binaries reproduce the remote collection counts from the
[previous investigation](../json-layout-and-admission/README.md).

The reproducer performs three warm-up and 24 measured parse/stringify calls on
the 17,765,942-byte `records_object_20m` fixture, discarding each result. At parse
call 8, both builds enter the eighth collection with 68,699,024 allocated bytes.
The checkpoint leaves 66,731,840; shared admission leaves 35,642,712: a
**31,089,128-byte (29.65 MiB) difference**.

At this collection, the checkpoint discovers a 145,000-element array in a stale
stack word at offset 920 inside `gc_check_trigger`'s native frame. Its address
matches the `records` field of the previous call's stringify input. Shared
admission has no discovery of that array in the corresponding scan. The full
collection runs at the pending-parse cleanup call near the beginning of
`parse_slow`, after the new input is rooted. A discarded 17,620,942-byte serialized
output is also found in the stack. These are conservative discoveries, not new
precise roots installed by JSON.

Controlled interventions at the already-validated mark-success instructions
identify the relevant stack region:

| Debugger treatment | Checkpoint full / minor | Shared admission full / minor |
|---|---:|---:|
| Observe only | 19 / 6 | 26 / 9 |
| Reject discoveries below the trigger frame | 19 / 6 | 26 / 9 |
| Also reject discoveries inside the trigger frame | 26 / 9 | 26 / 9 |
| Redirect precise minors to the existing full fallback | 28 / 0 | 28 / 0 |

The third treatment makes the checkpoint reclaim more at the first divergence
and subsequently collect more often. It is **not** a safe implementation proposal:
excluding an entire native frame can omit saved registers that contain legitimate
roots in other callers. Final output equality on this fixture is not a general
rooting proof.

The minor-entry probe reveals a second source of unnecessary work. At a precise
loop safepoint, after the round-trip helper has returned, a minor promotes
580,000 objects / 28,992,000 bytes. Five dirty old objects supply 390,760 slots
to scan. Large arrays allocated directly in old generation, including abandoned
growth prefixes, hold young children alive even when the arrays themselves are
dead. This is ordinary remembered-set retention; these observations do not prove
an untraced promotion shortcut is responsible.

Redirecting the compiled `!gen_gc_enabled()` branch to its existing full fallback
at these precise safepoints, while leaving generational GC enabled globally,
avoids those promotions. Both binaries then read 3,478,859 pointer slots in total,
versus 32,916,847 and 37,074,055 in the observation runs. That is approximately
89% and 91% less slot scanning. This diagnostic forces 24 full collections and
does not qualify a policy for retained objects, pause latency, CPU or RSS.
The branch jump uses the compiler's own argument packing and return path; the
minor and full function-entry ABIs are not interchangeable.

The existing survivor-promotion handoff only measures active survivor space,
not fresh Eden objects. Broadening it requires care: the existing handoff latch
prevents repeated full collections that cannot reclaim live survivors. A more
local opportunity is to stop publishing duplicate edges from unpublished JSON
array growth buffers and retire their obsolete payload after copying. That can
reduce construction and later scan work without changing collection pacing.

All ten preserved runs exit successfully, execute 27 parses, and match Node's
SHA-256 for the final 17,620,942-byte serialized output. The lifetime worker also
checks scalar output lengths throughout the loop. Only the last full output is
hashed by the debugger; this does not establish every iteration's content or
general semantic equivalence.

`summary.json`, per-run raw events, GC traces, driver logs, exact binary hashes
and symbol disassemblies preserve the evidence. To repeat, reconstruct the pinned
checkpoint and shared-admission lifetime workers from the preceding reports,
adjust only their locations in each `config.json`, place the original fixture
beside `debug_roots.py`, and run that driver on macOS with LLDB launch access.
The scripts assert binary and output hashes; offsets are specific to those
Mach-O binaries. Do not apply them to another build.

Run `python3 benchmarks/json_performance/results/json-root-retention/verify.py`
to validate the recorded counts, pointer identity, interventions and output
checks. This investigation does not update the Node/Bun ranking or satisfy the
remaining all-row CPU/RSS and no-regression objective.
