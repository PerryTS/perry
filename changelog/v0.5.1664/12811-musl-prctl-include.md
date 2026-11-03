The musl release builds compile perry-runtime again: `perry_memory_profile.c`
no longer includes `<linux/prctl.h>`, which musl-gcc cannot find (no kernel
headers). `<sys/prctl.h>` provides the THP prctl constants on glibc and musl.
