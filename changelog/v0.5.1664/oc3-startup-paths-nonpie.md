Startup: module paths register without filesystem I/O, and native glibc Linux
executables link at a fixed base (non-PIE), like bun.

**Path registration.** Each compiled module's key is `std::fs::canonicalize` of
its source path, computed by the compiler when it collects the module (the key
of `ctx.native_modules`, the CommonJS wrapper's partial/final publications and
`__filename` all carry it). The runtime canonicalized every one of those keys
again at startup: OpenCode `--version` registered 6,877 path initializers and
issued 19,676 `readlink` calls (95% of its syscall time). On any machine other
than the build host those paths do not exist, so the walk answered nothing.
Registration now stores the key verbatim. Only spellings computed at run time
canonicalize, and a spelling that already is a registered key names that
module without touching the disk, on any machine. One spelling per module
(#10851) holds because keys are canonical by construction and every other
spelling still goes through `std::fs::canonicalize`. The per-path and
per-directory canonicalization memos existed only for the registration volume
and are removed.

**Non-PIE.** As a PIE, every pointer in perry's static data was a relative
relocation applied by ld.so at each start: 1.74M RELR entries for OpenCode,
dirtying 32 MB of `.data.rel.ro`. Linked with `-no-pie` they are final at link
time. Runtime consumers of image addresses read the load bias
(`dl_iterate_phdr`, `dladdr`), and the fixed base 0x400000 is above the NaN-box
handle band (0x200000). The `-z pack-relative-relocs` probe is removed: there
are no relative relocations left to pack.

OpenCode on qb8, vs main 30853fcf9 (n=11 wall, n=3 counters):

| | base | this | delta |
|---|---|---|---|
| `--version` wall | 1319 ms | 1208 ms | -112 ms (-8.5%) |
| `--help` wall | 1515 ms | 1392 ms | -123 ms (-8.1%) |
| `--version` instructions:u | 6.775 G | 6.709 G | -0.98% |
| `--version` page faults | 53.3 k | 44.6 k | -16.3% |
| `--version` syscalls | 23,257 | 3,954 | readlink 19,676 -> 150 |
| loader (`ldd -r` mode) | 29.9 M instr, 10.4 k faults, 47 ms | 2.5 M, 522, 4 ms | |
