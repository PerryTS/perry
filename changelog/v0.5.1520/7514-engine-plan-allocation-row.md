### Docs: engine plan gains the object-construction row; JSON roadmap retired

The #7469 mutator campaign's symbolicated decomposition was invisible to
`docs/engine-plan.md` — the performance backlog had no object-allocation row
and referenced none of #7469/#7510/#7511/#7512, so the best-measured part of
the engine was absent from the plan that sequences work.

The plan now leads its performance section with object construction: the
`churn` variant decomposition (which isolates `push_num` at 2.7× to show the
array machinery is fine, putting ~79% of `churn` in construction), and the
group profile showing **~76% of construction is GC/feedback bookkeeping
against 7.7% for the allocation itself**. "What is left" is re-ordered to put
the 33.6% layout side-table lever (#7510) first and the #7512 bisect ahead of
the write-barrier work (#7511) it might account for; repsel's remaining
consumer work is explicitly sequenced after the bookkeeping levers, since
element reads are the best ratio in the table.

Two traps are recorded inline because both have already cost time:
`PERRY_WRITE_BARRIERS=0` cannot bound barrier cost (it also switches the
collector out of evacuating mode, making the benchmark *slower*), and a TS
annotation is never a layout fact — elision must be by-construction, and even
a static layout declaration can be revoked at runtime (#7501).

`docs/memory-perf-roadmap.md` is marked superseded: its goal was met (the
roundtrip win, #7476) and its standings are v0.5.211 against a v0.5.1289 HEAD.
