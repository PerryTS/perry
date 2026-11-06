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
