/* Linked with the production policy object and the pinned mimalloc source.
 * Allocate in a constructor before main to catch policies applied too late. */
#include <mimalloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/prctl.h>

extern void perry_retain_memory_profile_init(void);
extern long perry_memory_profile_allow_thp(void);
static void *early_allocation;
static int early_thp_disabled = -1;

__attribute__((constructor(102)))
static void allocate_before_main(void) {
    early_allocation = mi_malloc(64);
    if (early_allocation != NULL) {
        ((volatile unsigned char *)early_allocation)[0] = 42;
    }
    early_thp_disabled = prctl(PR_GET_THP_DISABLE, 0, 0, 0, 0);
}

int main(int argc, char **argv) {
    perry_retain_memory_profile_init();
    if (argc != 3 || early_allocation == NULL) return 2;
    const long option = perry_memory_profile_allow_thp();
    printf("allow_thp=%ld early_thp_disabled=%d\n", option, early_thp_disabled);
    mi_free(early_allocation);
    return option == strtol(argv[1], NULL, 10) && early_thp_disabled == atoi(argv[2]) ? 0 : 1;
}
