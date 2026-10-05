Generated stores into compiler-managed GC roots no longer emit an
incremental-mark shading gate and out-of-line call at every assignment.
Budgeted collections already rescan every local and module-global root during
their final remark, while synchronous collections expose no mutator window;
heap stores, runtime-owned roots, weak reads and allocation coloring keep their
existing barriers. This removes a major source of repeated cold code in large
functions without changing relocations or GC root rewriting.
