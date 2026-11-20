### Changed

- Codegen reads the runtime call-effect table (which runtime calls can collect or unwind) from a per-compilation `GcEffects` value carried in `CompileOptions`, replacing the process-global table. It is still built from the merge of the committed per-target tables, so emitted IR is byte-identical. The table digest is now part of every module object-cache key. (GC effects at build, slice 3.)
