x86-64 Linux: the GC root scan no longer walks the stack with the system
unwinder. JS frames step by their frame record (two loads, exact `sp` and
`rbp`); Rust, shim and foreign frames between them step by their unwind row
through the runtime's own direct decoder; the walk ends after `main` or at the
worker base trap. The back-edge poll, `gc()` and the inline allocation slow
paths are naked shims that hand their caller's frame to the collections they
run, so those walks start at the JS frame and never step the collector's own
frames (allocation offers it only on the arena slow path, the poll only
when it collects). `match_records` rejects a pc past a function's last record in one
search. On binary-trees a walk drops from 9.1 frames and ~28.7k instructions
to 1.1 frames and ~8.5k; `_Unwind_Find_FDE` is no longer called by any scan.

A row the walk cannot follow fails closed to `_Unwind_Backtrace`, and when
that also stops before the end of the stack the process now aborts naming the
module, instead of silently scanning a truncated stack.
