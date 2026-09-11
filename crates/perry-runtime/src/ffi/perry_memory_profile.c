/* Use mimalloc's actual header: libmimalloc-sys 0.1.49 does not expose the
 * allow_thp enum member in Rust, and its numeric id is not a stable API. */
#include <mimalloc.h>
#include <stdlib.h>
#include <string.h>

/* Run before mimalloc's default-priority constructor and Rust startup.
 * getenv/strcmp/mi_option_set_default do not allocate. The default setter
 * preserves mimalloc's own environment parser and explicit operator settings. */
__attribute__((constructor(101)))
static void perry_apply_small_process_default(void) {
    const char *profile = getenv("PERRY_MEMORY_PROFILE");
    if (profile != NULL && strcmp(profile, "small") == 0) {
        mi_option_set_default(mi_option_allow_thp, 0);
    }
}

/* A referenced symbol pulls this member and its constructor from the archive. */
void perry_retain_memory_profile_init(void) {}

/* Used by the fresh-process policy probe; dead-stripped from ordinary apps. */
long perry_memory_profile_allow_thp(void) {
    return mi_option_get(mi_option_allow_thp);
}
