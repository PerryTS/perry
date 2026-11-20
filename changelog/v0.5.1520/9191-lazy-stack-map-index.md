`js_gc_init` decoded the whole gc-map section at every process start. For
claude-code that is 17.4 MB, 72,713 functions and 2,151,926 records, producing a
117 MB index — and a run that never collects never reads a byte of it.

The build moves to the three functions every collection funnels through
(`gc_collect_minor_with_trigger`, `gc_collect_forced_evacuating_minor`,
`gc_collect_full_mark_sweep_with_trigger`), which is as late as it can go:
allocation is still legal there, and the root scan itself must stay
allocation-free once the collector owns the heap. `initialize` re-arms rather
than builds, so a newly loaded image's section is decoded by the next
collection. `PERRY_LAZY_STACK_MAPS` switches one binary between the two.

On a 1,500-function program carrying 3.87 MB of source, medians of interleaved
pairs of 40 runs: startup 12.77 ms vs 17.25 ms (−26%), peak RSS 35.6 MB vs
61.4 MB (−42%). A hello-world is unchanged, which is the expected shape — its
section is small, so there was little to defer. Scope worth stating: a
non-collecting run avoids the decode entirely, while a run that does collect
only has it deferred.

This is step 2 of the sequence #9182 opened, and the reason that PR landed
first. Scanning against an unbuilt index is indistinguishable from an image with
no native roots, so correctness rests on every path into the root scan passing
through the build — an enumeration that could not be established by reading the
code. The fail-closed assert caught three unenumerated entry points and one
misplacement: the build had been put in `gc_finish_arena_trigger_collection` and
`gc_finish_malloc_trigger_collection`, which take a `GcCollectOutcome` and
therefore run *after* the collection they name — useless, invisible to every
timing test, and a silent heap corruption weeks later. It aborted loudly in
minutes instead, four separate times.
