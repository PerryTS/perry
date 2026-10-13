Regenerate the macOS aarch64 GC call-effects table from release runtime and
stdlib archives built on macOS, restoring the archive-derived table as the
authority after recent runtime additions. This is a tables-only refresh.

The refresh adds 221 missing exports: 212 from the pristine builtin-prototype
change (211 `Reenters`, one `Leaf`), seven typed-family exports (five `Leaf`,
two `Reenters`), and two handle NaN-boxing exports (`Leaf`). It also classifies
`js_ctor_return_override` as `ThrowOnly` instead of `Reenters`. The typed-array
contract row added by hand remains `Leaf` in the generated archive evidence.

The Windows cross-build was attempted with cargo-xwin and again with the
existing OpenSSL stub setup. Both are blocked by the Windows length type in
`object/builtin_proto.rs:503`: `libc::write` requires `u32`, but the diagnostic
passes `usize`. The Windows table was not regenerated; resolving that Rust
build error belongs in a separate change.

The macOS regeneration and strict `--check` agree on all 4,334 symbols.
The effective most-conservative class across Linux, macOS, and Windows is
unchanged for every symbol (zero weaker classes, zero stronger classes).

Linux release archives also match all 4,339 committed rows in both the base
and candidate builds. Single-threaded library comparisons (excluding the
readable rooted-GC tests) reproduce exactly the same 14 codegen failures and
14 runtime failures on each arm; there are no new library failures. The
classifier's three-format collecting-call negative controls pass.

The full gap comparison records 1,927 cases per arm, with 1,817 raw passes
on each. Five timeout differences pass serial rechecks on both arms. The
other differences are a missing better-sqlite3 Node oracle and variable Node
ordering of worker error/message events; both Perry arms emit the same events.
There are no new gap failures attributable to this table refresh.

The n=5 interleaved, THP-disabled comparisons match Node for all 12 real
programs, both binarytrees variants, and all four class micros at N=1M and
N=5M, including buffer-heavy and worker-heavy on this base. All emitted
`.text` sizes are unchanged. Instruction deltas stay within measured variation.
Worker-heavy's +0.54% median sits inside 1.2–1.4% trial ranges; its variable
completion order distributes jobs across worker heaps differently. Repeated
GC diagnostics give 40–42 full collections, with median 41 on both arms.
