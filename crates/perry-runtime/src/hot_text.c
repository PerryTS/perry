// Only the linker's selected text prefix is backed with huge pages.
// This is a separate native object so unused code and literals never enter
// an unordered executable. All original bytes and virtual addresses survive.
#define _GNU_SOURCE
#include <stdint.h>
#include <stddef.h>

#ifdef __linux__
#include <fcntl.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#define HP ((size_t)2 * 1024 * 1024)
#ifndef MADV_COLLAPSE
#define MADV_COLLAPSE 25
#endif

// One bounded policy scan, without adding a strstr dynamic import to tiny
// ordered programs. Constant-size memcmp is inlined by the native compiler.
static int selected_thp_policy(const char *policy, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (policy[i] != '[') continue;
        if (len - i >= 8 && memcmp(policy + i, "[always]", 8) == 0) return 1;
        if (len - i >= 9 && memcmp(policy + i, "[madvise]", 9) == 0) return 1;
    }
    return 0;
}

void js_back_hot_text(const unsigned char *start_ptr,
                      const unsigned char *limit_ptr) {
    uintptr_t start = (uintptr_t)start_ptr, limit = (uintptr_t)limit_ptr;
    // Keep ordinary ELF alignment. After PIE relocation, only complete huge
    // pages strictly inside the selected code may be backed. Partial boundary
    // pages stay file-backed; small prefixes return before policy I/O or copy.
    if (!start || limit <= start || start > UINTPTR_MAX - (HP - 1)) return;
    start = (start + HP - 1) & ~(uintptr_t)(HP - 1);
    limit &= ~(uintptr_t)(HP - 1);
    if (limit <= start) return;

    // Honor system and process THP disablement. Avoid allocator activity.
    int fd = open("/sys/kernel/mm/transparent_hugepage/enabled", O_RDONLY | O_CLOEXEC);
    if (fd < 0) return;
    char policy[128];
    ssize_t n = read(fd, policy, sizeof policy - 1);
    close(fd);
    if (n <= 0) return;
    policy[n] = 0;
    if (!selected_thp_policy(policy, (size_t)n)) return;
    int mode = prctl(PR_GET_THP_DISABLE, 0UL, 0UL, 0UL, 0UL);
    // Mode 3 disables automatic THP but explicitly allows MADV_HUGEPAGE /
    // MADV_COLLAPSE (PR_THP_DISABLE_EXCEPT_ADVISED). Perry's allocator uses
    // that policy. Respect full disablement and unsupported/unknown modes.
    if (mode != 0 && mode != 3) return;

    size_t len = limit - start;
    void *dst = (void *)start;
    // File collapse avoids anonymous copying on supporting kernels/filesystems.
    if (madvise(dst, len, MADV_COLLAPSE) == 0) return;
    if (len > SIZE_MAX - HP) return;
    size_t reserve_len = len + HP;
    void *reserve = mmap(NULL, reserve_len, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (reserve == MAP_FAILED) return;
    uintptr_t base = (uintptr_t)reserve;
    uintptr_t aligned = (base + HP - 1) & ~(uintptr_t)(HP - 1);
    if (aligned > base) munmap(reserve, aligned - base);
    size_t tail = base + reserve_len - aligned - len;
    if (tail) munmap((void *)(aligned + len), tail);
    void *tmp = (void *)aligned;
    if (madvise(tmp, len, MADV_HUGEPAGE) != 0) {
        munmap(tmp, len);
        return;
    }
    memcpy(tmp, (const void *)start, len);
    // Best effort synchronous collapse if fault-time allocation used 4K pages.
    madvise(tmp, len, MADV_COLLAPSE);
    if (mprotect(tmp, len, PROT_READ | PROT_EXEC) != 0) {
        munmap(tmp, len);
        return;
    }
    // Prepare a complete RX mapping, then replace the original in libc.
    // No RWX window, no copy from unmapped text; failure preserves the original.
    if ((void *)syscall(SYS_mremap, tmp, len, len, MREMAP_MAYMOVE | MREMAP_FIXED, dst) == MAP_FAILED)
        munmap(tmp, len);
}
#else
void js_back_hot_text(const unsigned char *start, const unsigned char *limit) {
    (void)start;
    (void)limit;
}
#endif
