Fixed ELF links that died with `undefined reference to
<futures_channel::mpsc::SenderTask>::notify` out of a well-known wrapper
archive when a compiled package pulled in `bundled-streams`.

GNU `ld` scans each archive on the command line exactly once, left to right,
and the perry archive block is mutually recursive: the wrapper archives listed
*before* perry-stdlib resolve nothing (the user objects reference only stdlib
symbols), every wrapper member is pulled from the repeat *after* stdlib on
references stdlib itself opened, and those members then reference back into
stdlib — which `ld` has already walked past. That stayed invisible until
shared-dependency pruning correctly dropped a bundled member because stdlib
exports it. The archive block is now wrapped in `-Wl,--start-group` /
`-Wl,--end-group` on ELF targets so the linker re-scans it to a fixed point.
Symbol precedence is unchanged and non-ELF link lines are untouched; a link
that already resolved produces a byte-identical executable.
