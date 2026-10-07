/* Use mimalloc's actual header: libmimalloc-sys 0.1.49 does not expose the
 * allow_thp enum member in Rust, and its numeric id is not a stable API. */
#include <mimalloc.h>
#include <sys/prctl.h>
#include <linux/prctl.h>
#ifndef PR_THP_DISABLE_EXCEPT_ADVISED
#define PR_THP_DISABLE_EXCEPT_ADVISED (1UL << 1)
#endif

/* GC backing belongs to Perry regions. Ordinary native allocations use
 * mimalloc with process-wide THP disabled before its startup constructor.
 * Explicit allocator options cannot re-enable sparse native huge pages. */
__attribute__((constructor(101)))
static void perry_apply_small_process_default(void) {
    mi_option_set(mi_option_allow_thp, 0);
    /* mimalloc normally sets the full process disable bit. Establish the
     * advised-only state first: its OS initialization observes a nonzero
     * PR_GET_THP_DISABLE and leaves it intact. No allocator patch or heap
     * allocation here. Older kernels reject this flag and retain the safe
     * all-base-pages fallback when mimalloc initializes. */
    /* Preserve an inherited strict operator disable (also the all-THP-off
     * measurement control). Never relax a parent's explicit process policy. */
    if (prctl(PR_GET_THP_DISABLE, 0UL, 0UL, 0UL, 0UL) != 1) {
        if (prctl(PR_SET_THP_DISABLE, 1UL, PR_THP_DISABLE_EXCEPT_ADVISED, 0UL, 0UL) != 0) {
            (void)prctl(PR_SET_THP_DISABLE, 1UL, 0UL, 0UL, 0UL);
        }
    }
}

/* A referenced symbol pulls this member and its constructor from the archive. */
void perry_retain_memory_profile_init(void) {}

/* Used by the fresh-process policy probe; dead-stripped from ordinary apps. */
long perry_memory_profile_allow_thp(void) {
    return mi_option_get(mi_option_allow_thp);
}
