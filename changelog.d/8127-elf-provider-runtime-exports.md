`gc-native-roots` is green on ELF again. The gate failed on both Linux
platforms — Mach-O and Windows passed — with "stdlib provider is bound to a
different runtime image". Re-running the last-good job on its own commit shows
that commit still passes, so the environment was never at fault: the break came
with the ELF branch of the #8075 linker shim added in #8089.

That branch wrote a version script listing the 16 provider exports followed by
`local: *`. The provider statically links the runtime rlib as well as loading
the runtime `.so`, so it carries its own `js_gc_init` and friends; `local: *`
binds those internally, and a local symbol is not preemptible. The stdlib
therefore stopped resolving stateful runtime calls to the image the host loaded
first — the exact condition the fixture exists to detect.

The ELF version script now keeps the runtime's symbols global, as the Mach-O
branch already did by re-exporting `nm`'s view of the runtime library.
`local: *` still hides everything else, so #8089's intent — export the Web
Fetch/Streams surface the later-loaded app needs and nothing more — is
unchanged. ELF reads the DYNAMIC symbol table (`nm -D --defined-only`), since
preemption is governed by what the dynamic linker sees.
