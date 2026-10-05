/* Separate archive member: only generated iterator-cleanup pads pull it in.
 * The common trap's weak reference must not retain an unused personality. */
#include <stdint.h>
#if defined(__linux__) && defined(__x86_64__) && defined(__GLIBC__) && __SIZEOF_POINTER__ == 8
extern int perry_native_iterator_cleanup_before_trap(void);
int perry_iterator_cleanup_query(void) {
    return perry_native_iterator_cleanup_before_trap();
}
extern int perry_eh_personality(int, int, uint64_t, void *, void *);
int perry_iterator_eh_personality(int version, int actions, uint64_t cls,
                                 void *exception, void *context) {
    return perry_eh_personality(version, actions, cls, exception, context);
}
#endif
