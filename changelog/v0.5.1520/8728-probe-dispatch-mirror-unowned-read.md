Fixed the intermittent failure of
`object::native_call_method::probe_dispatch_tests::the_magic_screen_covers_every_symbol_and_no_ordinary_object`
(#8728), which read memory the test does not own.

**Root cause.** The test's closing loop mirrors the `obj_type` `match` at the
tail of `gc_pointer_and_type_from_value` (`native_call_method.rs`), as a
negative control: it exists to show that *no arm of that match* would exclude a
`Box`-leaked symbol, so the `classify(sym).is_none()` assertions above it are
proving the `SYMBOL_MAGIC` screen is load-bearing rather than passing by
accident. To pick a `match` arm it read `obj_type` out of a `GcHeader` at
`sym - GC_HEADER_SIZE` — but a leaked symbol has **no `GcHeader`**. It is a
plain `Box::leak` of a 24-byte `SymbolHeader` (`symbol/constructors.rs`,
`js_symbol_for`), so those eight bytes are allocator metadata or a neighbouring
allocation's tail. The test's own comment four lines above said exactly that
("bytes ... belong to the allocator and not to us") while the code went ahead
and read them.

That read is not merely unowned, it is load-bearing on garbage. The
`GC_TYPE_REGEXP` arm is a bare `true` — it never consults the address — so
whenever the stray byte happened to equal `GC_TYPE_REGEXP` (20) the mirror
reported "this symbol would be excluded even without the screen" and the
assertion fired. Eight symbols are sampled per run, which is why it presented
as a low-single-digit-percent flake rather than a hard failure.

**Fix.** The mirror now takes `obj_type` as a *parameter*
(`excluded_by_the_header_arms`) and the test quantifies over the entire domain
of that byte instead of sampling the one value that happens to be at
`sym - 8`, asserting that the set of `obj_type` values for which production
would exclude the symbol is exactly `{GC_TYPE_REGEXP}`. No unowned memory is
read, the result is deterministic, and the statement is strictly stronger than
the single-sample version it replaces: the old code checked one of 256 possible
bytes, the new code checks all 256.

The mirror is kept **faithful** rather than "improved". A regexp predicate does
exist (`regex::is_registered_regex`, `regex.rs`), but production deliberately
does not call it — RegExp has its own GC kind, so the header is treated as
authoritative — and calling it here would both prove something about a function
this dispatch path never invokes and reintroduce an `addr - 8` read, since it
reaches `try_read_gc_header`. The two arms that *do* consult the address
(`set::is_registered_set`, `map::is_registered_map`) are registry-first and
dereference-free for an unregistered address (#4665), so the mirror can call
them safely on a symbol.

**The negative control is preserved and sharpened.** The assertion now fails in
both directions, which is the right way round: an added arm that covers leaked
symbols grows the excluding set, and giving the RegExp arm a real predicate
shrinks it — either way the mirror must be re-derived from production rather
than left to drift. The screen's load-bearingness is still pinned directly by
the two assertions at the top of the same test (`may_be_symbol_header(sym)` must
be true for every leaked symbol, and `classify(sym)` must be `None`), and by
`header_directed_dispatch_needs_the_symbol_magic_screen`, which sabotages the
screen and requires the probe to return.

**Reproduced, not inferred.** The test was instrumented to print the byte it
was reading at `sym - GC_HEADER_SIZE`, and the suite run repeatedly. Across 39
single-threaded full-suite runs that byte took **64 distinct values** over 273
draws — about half zero, the rest dominated by ASCII (`'a'`, `'e'`, `'i'`,
`'g'`), i.e. the tail of a freed `StringHeader` — and on run 30 the window was
`173, 0, 0, 20, 151, 0, 76`: the fourth symbol read exactly `20`, the value that
fires the old assertion. Run the test *alone* and the byte is a stable `0`
(1000/1000 passes, and a 320-symbol sweep never left zero), which is why this
only ever failed as part of a full suite and why it reads as a flake: the value
is a function of what the preceding ~2600 tests left on the allocator's free
list, not of anything the test controls.

**Validated.** `perry-runtime --lib` single-threaded is 2673 passed / 0 failed /
4 ignored, unchanged. 100 consecutive full-suite runs on the fixed build: 100
green. 200 consecutive isolated runs of the test: 200 green. The negative
control was checked by sabotage rather than assumed — with the
`may_be_symbol_header` guard in `gc_pointer_and_type_from_value` made inert, the
rewritten test fails 5/5 at `assert!(classify(sym).is_none())`, so it still goes
red on a build where the screen does nothing.
