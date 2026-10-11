// Measurement-only linkage. GC timing counters are always-on; no diagnostic
// scanners or per-phase timers are enabled. Symbol aliases are generated from
// each arm's own freshly built runtime archive by the harness.
#include <stdint.h>
#include <stdatomic.h>
#include <stdio.h>
#include <sys/prctl.h>
#include <unistd.h>
#include "gc-symbols.h"
extern _Atomic uint64_t PERRY_LANE_PHASE_INS[10] __attribute__((weak));
extern _Atomic uint64_t PERRY_LANE_PHASE_US[10] __attribute__((weak));
extern _Atomic uint64_t PERRY_LANE_KEY_BIRTHS __attribute__((weak));
extern _Atomic uint64_t PERRY_LANE_KEY_BYTES __attribute__((weak));
extern void js_gc_stats(uint64_t *, uint64_t *, uint64_t *);
extern void __real_js_gc_release_current_thread_collection_side_allocations(void);
__attribute__((constructor)) static void disable_thp(void) {
    if (prctl(PR_SET_THP_DISABLE, 1, 0, 0, 0)) _exit(91);
}
void __wrap_js_gc_release_current_thread_collection_side_allocations(void) {
    uint64_t counts = 0;
    unsigned long huge_kb = 0, kb;
    char line[256];
    FILE *smaps = fopen("/proc/self/smaps", "r");
    if (!smaps) _exit(92);
    while (fgets(line, sizeof(line), smaps))
        if (sscanf(line, "AnonHugePages: %lu kB", &kb) == 1) huge_kb += kb;
    fclose(smaps);
    uint64_t minors = atomic_load_explicit(&obs_minors, memory_order_relaxed);
    js_gc_stats(&counts, 0, 0);
    uint64_t full = counts - minors, minor_pause = 0, full_pause = 0;
#ifdef KEYLISTS_PROCESS_TOTALS
    full = atomic_load_explicit(&PERRY_LANE_FULL_COUNT, memory_order_relaxed);
    minors = atomic_load_explicit(&PERRY_LANE_MINOR_COUNT, memory_order_relaxed);
    counts = full + minors;
    full_pause = atomic_load_explicit(&PERRY_LANE_FULL_PAUSE_US, memory_order_relaxed);
    minor_pause = atomic_load_explicit(&PERRY_LANE_MINOR_PAUSE_US, memory_order_relaxed);
#endif
    fprintf(stderr, "[keylists-observe] collections=%lu minor=%lu full=%lu huge_kb=%lu step_us=%lu remark_us=%lu minor_us=%lu full_us=%lu full_pause_us=%lu minor_pause_us=%lu gc_pause_us=%lu\n",
            counts, minors, full, huge_kb, atomic_load_explicit(&obs_step, memory_order_relaxed), atomic_load_explicit(&obs_remark, memory_order_relaxed), atomic_load_explicit(&obs_minor, memory_order_relaxed), atomic_load_explicit(&obs_full, memory_order_relaxed), full_pause, minor_pause, full_pause + minor_pause);
    if (PERRY_LANE_PHASE_INS) {
        fprintf(stderr, "[keylists-phases] births=%lu bytes=%lu", atomic_load(&PERRY_LANE_KEY_BIRTHS), atomic_load(&PERRY_LANE_KEY_BYTES));
        for (int i = 0; i < 10; ++i)
            fprintf(stderr, " phase%d_ins=%lu phase%d_us=%lu", i, atomic_load(&PERRY_LANE_PHASE_INS[i]), i, atomic_load(&PERRY_LANE_PHASE_US[i]));
        fprintf(stderr, "\n");
    }
    __real_js_gc_release_current_thread_collection_side_allocations();
}
