### Pace worker full GC by allocation bytes and buffer bursts

Worker heaps could retain dead buffers while net old-generation growth and malloc object counts stayed below their reclaim thresholds. Minor collections, promotion credit and reused holes hid churn, and several independently paced heaps could retain it at once.

Track bytes allocated since completed full reclaim, including generated/runtime arena bumps, reused holes, malloc growth and reported external backing. Exclude collector copies while counting mutator allocations between budgeted steps. Publish numeric heap debt in MiB batches to shared process pressure and subtract it at thread exit. At an outermost microtask boundary, substantial byte pressure or a large-buffer burst requests the existing precise reclaim path. Shared-pressure selection requires large-storage churn too, so nursery-only churn keeps its cheap minor route. Pay only the burst present at a full trace begun at the boundary. Preserve the opportunity across a mid-task full and for allocate-black births during a budgeted full. Live-set bands and productivity backoff limit repeated traces; nursery pacing and heap limits retain their defaults.

Regression units cover precise live roots and actual dead-buffer reclaim, minors, generated bumps, old-hole reuse, malloc/external growth, process debt retirement and the mid-task, post-snapshot and budgeted-window cases. No upm changes or new owner registry.

A worker about to park finishes an existing budgeted minor when it blocks an owed large-buffer reclaim, using the existing guarded stepper. Forced budgeted-old instrumentation retains its sliced sweep schedule.

Give an already owed full the precise boundary before the ordinary poll can start a redundant budgeted minor. A pump regression checks that this performs one collection rather than a minor followed by a full.

Price full-reclaim productivity by the net reduction in old, malloc and reported external residence. Cheap nursery garbage cannot reset mature reclaim backoff. A real-allocation regression exercises 64 MiB of dead nursery objects around a stable rooted old buffer.
