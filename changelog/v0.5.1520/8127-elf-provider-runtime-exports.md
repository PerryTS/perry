`gc-native-roots` is green on ELF again. The gate failed on both Linux
platforms — Mach-O and Windows passed — with "stdlib provider is bound to a
different runtime image". Re-running the last-good job on its own commit shows
that commit still passes, so the environment was never at fault: the break came
with the ELF branch of the #8075 linker shim added in #8089.

The cause is that branch's `local: *`. The provider statically links the
runtime rlib as well as loading the runtime `.so`, so it carries its own
`js_gc_init` and friends; `local: *` binds those internally, and a local symbol
is not preemptible. The stdlib then resolved stateful runtime calls to its own
copy rather than the image the host loaded first — the exact condition the
fixture exists to detect. Before the shim parsed `--version-script` at all the
rustc-generated script was passed through and the gate passed, so the
regression is the hiding, not the export list.

The version script now names the 16 provider exports with no `local: *`, so
every other symbol keeps its default global, preemptible binding. Re-exporting
the runtime's symbol table explicitly (as the Mach-O branch does via `nm`) is
not an option on ELF: rustc also passes `--no-undefined-version`, so naming a
symbol the output does not define is a hard lld error.
