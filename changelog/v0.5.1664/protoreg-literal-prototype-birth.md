Fix the allocation-light `Object.create` regression introduced by weak holder
memos (#12507). Module-singleton source-ordered literal prototypes now use the
existing old, never-move ordinary birth funnel, so inherited keyed reads and
method calls can prime immediately rather than waiting for a nursery collection.
Construction order, shape transitions and write barriers are unchanged; no
runtime cache, registry or side table is added. Add a parity witness covering
inherited string/symbol reads, methods, accessors, mutation and relinking.
