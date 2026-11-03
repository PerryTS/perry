The GC representation matrix now honors each fixture's existing `parity-env`
settings in the explicit `loop_polls` arm at both compilation and execution.
Seeded and protected witnesses therefore select the instrumented runtime through
the normal auto-optimize path. Fixture metadata is restricted to validated GC
settings; all other arms keep their original environments. Reports and progress
logs record each cell's compile and run assignments. A CI routing self-test
checks control isolation and rejects planted compile/run dispatch defects.

The call-argument and packed-global-cache witnesses now carry their recorded
seeded collection settings, including the 4 KiB allocation interval and protected
from-space. Their TypeScript workloads remain unchanged. This makes the existing
moving arm exercise these short fixtures instead of accepting oracle parity
without any collection; the per-cell movement check remains mandatory.
