`setTimeout` no longer pays for the realm global's bootstrap.

#10836 made timers return ordinary objects, so `js_set_*_callback*` ends with
`timer_object(id, kind)` and the first such call builds `Timeout.prototype` /
`Immediate.prototype`. That install cost **4–6 ms**, and the cost is not the
prototypes: it is `globalThis`.

`build_timer_prototypes` reaches `install_proto_method`, which records the spec
property descriptors for each method. The descriptor bookkeeping asks "is this
receiver `Object.prototype`?" — `array::prototype_addr::resolve_prototype_addr`
— and that question was answered by *materializing* the realm global, i.e. by
running `populate_global_this_builtins`. Per-step timing inside the first
`setTimeout` of the repro:

```
[timerprof] timer_object:alloc          164.8µs
[timerprof] Timeout:object_alloc         23.8µs
[gc-globalthis-bootstrap] elapsed_us=5475
[timerprof] Timeout:install_ref        5.687ms   <- the first install_proto_method
[timerprof] Timeout:install_rest          8.8µs
[timerprof] Timeout:install_constructor    1.6µs
[timerprof] Timeout:install_symbols        2.5µs
[timerprof] Immediate:*                  < 5µs each
[timerprof] timer_object:{link,meta}     < 1µs
```

Not a collection: the same run reports `cycle_starts=0 copying_minors=0
full_sync_us=0`. The prototypes themselves cost ~45 µs; the 5.5 ms is one
`js_get_global_this()` the timer path never needed.

Why it blocked merge train 248: a timer's deadline is `now + delay`, taken per
call. Six milliseconds inside call #1 pushes call #2's deadline six
milliseconds out, so `test_gap_6287_timer_batch_order`'s deliberately
reverse-ordered pair — `setTimeout(…, 10)` at T0 then `setTimeout(…, 5)` at
T0+6 — acquired deadlines T0+10 and T0+11 and fired in the wrong order
(`…,t10,t5,t7,…` against node's `…,t5,t7,t10,…`).

**Fix** — `resolve_prototype_addr` answers without building the world. Before
the realm global exists, neither `Array.prototype` nor `Object.prototype` has
been allocated, so no address can be one of them and the already-supported
"not resolved" answer (`0`) is the correct one; `bootstrap_prototype_addr` can
return it anyway and every caller handles it. The new predicate
`object::global_this_is_materialized()` reports whether this thread owns a
`globalThis` *without* creating one, and is deliberately true from the moment
the global object is allocated — before population finishes — so a caller that
runs *inside* `populate_global_this_builtins` (the descriptor bookkeeping does)
still reaches the bootstrap exactly as before.

The check sits in the inlined half of the accessor, not behind the
`#[inline(never)]` cold call: `note_array_index_write` consults
`array_prototype_addr()` on every indexed array write and the cell now stays
unresolved for as long as a program has no `globalThis`, so the cold half would
have traded a one-off 5 ms for an out-of-line call per write.

Hoisting `build_timer_prototypes` into runtime init was the other candidate and
is worse: it would move the same 5.5 ms onto *every* program's startup,
including the timer-free ones that pay nothing today (a program that only does
`console.log` and array work prints no `[gc-globalthis-bootstrap]` line at all).

**Measured** — `Date.now()` deltas between four successive `setTimeout` calls,
release build, macOS arm64, node 26.5.1 oracle:

| arm | deltas (ms) | `[gc-globalthis-bootstrap]` |
|---|---|---|
| train 248 (v0.5.1627, `5a053e72`) | `6,0,0,0` (`4,0,0,0` typical, `10,0,0,0` cold) | `elapsed_us=3632…5475` |
| with this fix | `1,0,0,0`, then `0,0,0,0` ×4 | not emitted |
| node 26.5.1 | `0,0,0,0` | — |

`run_parity_tests.sh --filter test_gap_6287_timer_batch_order` passes 3/3
(harness exit 0). The `test_gap_` slices matching `timer`, `text` and `encod`
are 4/4, 1/1 and 2/2. `timeout` matches one further test,
`test_gap_9592_child_timeout_threads`, which fails on macOS for an unrelated
reason: it spawns `/bin/true`, which does not exist on macOS, so the **node**
oracle exits 1 with `spawn /bin/true ENOENT`.

Regression fixture:
`timer::tests_inline::first_timer_cost_tests::the_first_timer_handle_does_not_bootstrap_the_realm_global`
asserts the property directly rather than through the batch ordering — it runs
`timer_object` on a thread with no realm global and requires that the call
install `Timeout.prototype` **and** leave `globalThis` unmaterialized. The
precondition (a fresh thread) is asserted, so a harness change that shares the
thread turns it red instead of making the verdict vacuous.
