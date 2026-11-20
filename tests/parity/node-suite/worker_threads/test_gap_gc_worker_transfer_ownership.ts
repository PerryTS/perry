// parity-env: PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=0.25 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_OLD_SWEEP=1
// parity-node-argv: --expose-gc
// Exercise the ownership witness with moving minors and protected swept pages.
import "./test_gap_worker_transfer_ownership.ts";
