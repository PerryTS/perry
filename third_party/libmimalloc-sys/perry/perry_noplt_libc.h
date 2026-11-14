/* perry: mimalloc's libc calls through the GOT, not the PLT.
 *
 * Force-included (`-include`) into mimalloc's unity TU only, by this crate's
 * build.rs on x86_64 glibc Linux with gcc (perry/noplt.rs, which also fails
 * the build if one of these calls is still a PLT call).
 *
 * Why: Rust code takes these functions' addresses from the GOT (GLOB_DAT,
 * bound at start), and mimalloc called them through the PLT. GNU ld routes
 * such a PLT call through `.plt.got` (the bound GOT entry); lld has no
 * `.plt.got` and gives each function a second JUMP_SLOT, bound lazily: about
 * 400 ld.so instructions on its first call (chess +6K per run). Only these
 * calls change: a global -fno-plt rewrites every call of the C parts and
 * moved enough text to cost peak RSS.
 */
#ifndef PERRY_NOPLT_LIBC_H
#define PERRY_NOPLT_LIBC_H
#if defined(MI_DEBUG) && defined(__linux__) && defined(__x86_64__) && defined(__GNUC__) && \
    !defined(__clang__) && defined(__has_attribute)
#if __has_attribute(noplt)

/* memcpy / memset: gcc emits these builtins as libcalls, which ignore
 * `noplt`. Renaming the builtin's symbol keeps it a builtin (inline
 * expansion, "returns its first argument") and sends the remaining library
 * calls to a local stub that jumps through the GOT entry, as GNU ld's
 * `.plt.got` stub does. Declared before any header so every later
 * declaration (glibc's fortified inlines too) inherits the name. */
void *memcpy(void *__restrict, const void *__restrict, __SIZE_TYPE__) __asm__("perry_mi_memcpy");
void *memset(void *, int, __SIZE_TYPE__) __asm__("perry_mi_memset");
__asm__(".pushsection .text.perry_mi_memcpy,\"ax\",@progbits\n"
        ".p2align 4\n"
        ".type perry_mi_memcpy,@function\n"
        "perry_mi_memcpy:\n"
        ".cfi_startproc\n"
        "  jmp *memcpy@GOTPCREL(%rip)\n"
        ".cfi_endproc\n"
        ".size perry_mi_memcpy, .-perry_mi_memcpy\n"
        ".popsection\n"
        ".pushsection .text.perry_mi_memset,\"ax\",@progbits\n"
        ".p2align 4\n"
        ".type perry_mi_memset,@function\n"
        "perry_mi_memset:\n"
        ".cfi_startproc\n"
        "  jmp *memset@GOTPCREL(%rip)\n"
        ".cfi_endproc\n"
        ".size perry_mi_memset, .-perry_mi_memset\n"
        ".popsection\n");

#ifndef _DEFAULT_SOURCE
#define _DEFAULT_SOURCE /* as mimalloc's static.c defines it before any include */
#endif
#include <features.h>
#if defined(__GLIBC__)
#include <errno.h>
#include <pthread.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/sysinfo.h>
#include <time.h>
#include <unistd.h>

/* The other functions mimalloc calls that Rust code also binds through the
 * GOT: an ordinary call honours `noplt`. */
#define PERRY_NOPLT(f) extern __typeof__(f) f __attribute__((noplt))
PERRY_NOPLT(__errno_location);
PERRY_NOPLT(clock_gettime);
PERRY_NOPLT(getrusage);
PERRY_NOPLT(madvise);
PERRY_NOPLT(mmap);
PERRY_NOPLT(mprotect);
PERRY_NOPLT(munmap);
PERRY_NOPLT(prctl);
PERRY_NOPLT(pthread_key_create);
PERRY_NOPLT(pthread_key_delete);
PERRY_NOPLT(pthread_setspecific);
PERRY_NOPLT(syscall);
PERRY_NOPLT(sysconf);
PERRY_NOPLT(sysinfo);
#undef PERRY_NOPLT
#endif /* __GLIBC__ */
#endif /* noplt */
#endif
#endif /* PERRY_NOPLT_LIBC_H */
