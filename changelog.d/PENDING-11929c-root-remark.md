Generated stores into compiler-managed GC roots no longer emit an
incremental-mark shading gate and out-of-line call at every assignment.
Budgeted collections already rescan every local and module-global root during
their final remark, while synchronous collections expose no mutator window;
heap stores, runtime-owned roots, weak reads and allocation coloring keep their
existing barriers. This removes a major source of repeated cold code in large
functions without changing relocations or GC root rewriting.

Inline class births now shade and seed once while incremental marking is
active, after their header/payload initialization and before publication.
This preserves allocate-black semantics in the sliced remembered-set and
weak-processing mutator windows after the last root remark. Regression
coverage includes 200 bound stable homes, fresh marking-window stores, and
both post-remark windows, with a lost-birth sabotage proof.
