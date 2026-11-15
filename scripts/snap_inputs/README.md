# Instrumented snapshot input evidence

trace.py OUTPUT -- setarch -R INSTRUMENTED-BINARY ... records native inputs
and successful once-guarded module returns. It requires Linux x86-64, ptrace,
hardware perf counters and an executable with the census ABI and symbols.
It never links into a production executable. PERRY_SNAP_CENSUS=REPORT
also writes the ordered semantic events to REPORT.inputs.tsv.

account.py REPORT OUTPUT ACCOUNT.json joins the counter domains and checks
every census instruction charge, rather than adding whole modules in entry order.
Here OUTPUT is the native JSONL from trace.py. Its instruction positions include
the recorder and tracer probes; they are not production instruction counts.

sites.py reconstructs the identity ledger from final facts dump site records
joined to their proven verdicts. Its usage is printed by an argument error.

Fixtures are diagnostic subjects. Keep compiler and runtime archives pinned
separately for the base and changed arm, compile each fixture's main.ts, then run
through the recorder to ROOT/fx/NAME/head/{report,native.jsonl,stdout}.
python3 check.py ROOT checks allinputs, r1 and r2. On the base, requiring the
ordered TSV must fail. r1 includes a dependency-phase parent. r2 writes an
outliving cell before entering a child. net exercises socket inputs.
stop is fresh normally and fails with SNAP21_FORCE_IMAGED_PID=1.

replay is a counterexample component model, not an image loader. Normally it
passes. SNAP21_UNSAFE_REPLAY=R1 repeats a body effect when a body-started frame is
misclassified as a dependency driver. SNAP21_UNSAFE_REPLAY=R2 repeats a persistent
write. Both must exit 1 while the child's once guard prevents a second child run.
These reds do not establish end-to-end snapshot correctness. That requires a
real image loader, relocation and guard-mismatch tests in a later phase.

The first changed tracked word is conservative evidence, not a proof of semantic
writes or their absence: representation writes, non-GC native state and local
allocations must be audited separately. A decoder's "representation" annotation
likewise cannot admit a native resource. Unknown input or incomplete payload is
a STOP; module names and TypeScript types never grant eligibility.

SNAP22 abort survival: compile `fixtures/abort/main.ts` with the instrumented
cohort, then run with `SNAP22_ABORT_SENTINEL=before_abort` and
`PERRY_SNAP_CENSUS=<report>`; expect SIGABRT/134. Run
`python3 scripts/snap_inputs/check_abort.py <report>.inputs.tsv`.
The assertion is RED with SNAP21's boundary-buffered recorder. Each row now
uses native write-all immediately, including first-write rows from signal
context. Native logging intervals carry lseek marker 0x5322; the syscall
companion excludes only those explicit intervals and their dedicated fds.
No JS values, shapes, native frames or event payload buffers are retained
by the recorder. A failed or oversized write fails explicitly (92/93).

The recorder suppresses allocator callbacks with const-initialized native TLS
before callback formatting or borrowing census state. Only diagnostic scopes
are suppressed; the actual observed JS/native operation runs outside them.
Clock and libc environment observation payloads use bounded stack buffers.

For the GC/re-entry regression, compile fixtures/effect/main.ts (requires the
same installed effect package as the subject) and fixtures/pressure/main.ts.
Run each with PERRY_SNAP22_WATCH=0 and =1, PERRY_SNAP_NOPROTECT=1 and distinct
report paths. check_recorder.py ROOT [ARM=guard-head] asserts six ordered
prefixes under ROOT/fx/{effect,shapes,abort}/ARM, report-0/report-1 inputs,
stdout-0/stdout-1, and a live-object census proving GC completed. The abort
fixture intentionally exits 134 without reaching the boundary. On the old
recorder the effect GC crashes, pressure fails, and abort rows are absent or
out of order; a successful process exit alone cannot satisfy these assertions.

Protection consistency regression: compile `fixtures/protection/main.ts` with
an instrumented cohort, then run `bash scripts/snap_inputs/protection_fixture.sh
BINARY OUTPUT base|head`. A dynamic child changes a cell born before its import,
then accesses other pages. On the base the first-write watcher triggers a
rule-4 conflict, bulk protection closes unrelated pages, and the process faults
although its page policy is RW. Watch=0 finishes; the fix finishes in both modes.
The checker requires the nonzero first-write row, exact output, and the native
mprotect evidence. It also rejects any bulk PROT_NONE in this NOPROTECT fixture.
