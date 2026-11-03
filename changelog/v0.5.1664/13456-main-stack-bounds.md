Linux main-thread stack bounds come from the kernel stack layout and RLIMIT_STACK instead of glibc parsing /proc/self/maps, removing about 130k instructions from every startup that allocates.
