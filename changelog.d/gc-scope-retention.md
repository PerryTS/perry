Fix closure scope over-retention by preserving bare blocks as distinct HIR
statement lists through lowering and scope grouping. Bare blocks now use the
existing unconditional branch representation; LLVM folds the constant branch
after grouping. An outer closure no longer shares a scope record with captured
mutable bindings declared in an inner bare block. Bindings in the same lexical
activation still share their record. No runtime caches or side tables are added.

Lowering tests cover function and module bare blocks, with a same-scope control.
A scope-map test pins the separate capture records and proves that flattening
the lists recreates the erroneous shared record. The existing scope-retention
gap test remains unchanged, including its captured-binding keepalive control.
