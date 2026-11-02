**The GC's allocator hand-back addressed the wrong allocator, and an idle
compiled TUI held ~318 MB because of it.**

`run_malloc_trim` calls `libc::malloc_trim(0)` on glibc and
`malloc_zone_pressure_relief` on macOS. Neither reaches mimalloc, which
`lib.rs` installs as the `#[global_allocator]` on every 64-bit target — so on
both production platforms the step ran, reported success, and could not touch
the pages a collection had just freed. Measured on the compiled claude-code
TUI at idle (Linux): three explicit full collections each executed
`malloc_trim(0)` in 16-23 us and moved RSS by 0 MB, while the same collection
under `MIMALLOC_PURGE_DELAY=0` returned 318 MB. The collector was already
freeing the memory; mimalloc was holding it.

A major collection now also calls `mi_collect(true)` at the same point
(Reclaim, outside the atomic tail), which is the primitive that reaches this
process's allocator. Major-only, so the cost is paid once per full cycle
rather than on every free the way the env knob would. Kill switch:
`PERRY_GC_MALLOC_PURGE=0`. `libmimalloc-sys` moves from an Apple-only target
dependency to every 64-bit target; the Apple-only part was always the VM-tag
retag (#6882), not the crate.

Two side tables shrink at the same point, for the same reason — hashbrown
never releases on `remove`/`retain`, so both kept the allocation of their
startup PEAK for the life of the process. At idle on the same binary the
shape facts index held 30.8 MB at 12.3% fill and the descriptor table 13.8 MB
at 10.5% fill, and the malloc-object registry held 9.4 MB at 2.3% fill behind
a one-way `heavy_capacity_reserved` latch that nothing ever cleared. Both now
`shrink_to(2 * len)` once per major collection, keeping one doubling of
headroom so an oscillating table does not re-grow on the next insert.

Separately, codegen's function names and source text are `private
unnamed_addr constant` globals in the program image: read-only, file-backed,
already resident at zero private cost. `js_register_function_name` and
`js_register_function_source` copied all of it into `Arc<[u8]>` at module
init, making a second dirty copy — 5.1 MB of names and 23.8 MB of source on
the same binary. Both registries now borrow the image. Runtime-inferred names
(a symbol description, `get <key>`) stay owned through a two-variant value, so
nothing about `fn.name` or `Function.prototype.toString` changes; the FFI
safety contracts now require `'static` instead of promising a copy.
