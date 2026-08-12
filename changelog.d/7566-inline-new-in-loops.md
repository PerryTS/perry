**`new` sites inside a loop now take the inline bump allocator; everything else
keeps the outlined call (#7469 workstream A).**

The outlined per-`new`-site allocator has been the default since the `[#bloat]`
work: it collapses ~145 lines of per-class-constant IR per site into a single
`js_object_alloc_class_inline_keys` call. **The size half of that decision still
holds** — measured at ~268 bytes of machine code per site (+0 / +49,536 /
+214,656 bytes across 10 / 200 / 800-site programs).

**The speed half has inverted.** The comment justifying it reads *"~17% faster
on an 8M-allocation loop (the inline bump bloated the hot loop, hurting
icache/regalloc more than the saved call)"*. Today the outlined form is **1.81×
slower** on `churn_alloc` and 1.78× on `push_cls`. Nothing about the inline bump
changed — everything *around* the allocation got cheaper (#7474, #7486, #7487,
#7501, #7525, #7532, #7535, #7536, #7552), so the surviving FFI call and the
thread-local resolutions it performs now dominate what its code bloat costs.

Those resolutions cannot be made cheaper on this platform, which is worth
recording so it is not attempted again: **Mach-O has no local-exec TLS model.**
Building the entire runtime with `-Ztls-model=local-exec` leaves the `blr`
through the TLV descriptor in `layout_forget_object` byte-identical and moves
`churn_alloc` by 1.02×. Per-call price is already at the plain-global floor
(1.29 ns for Rust's `const`-init `thread_local!`, 1.24 ns for a plain global,
1.31 ns for C `__thread`; `pthread_getspecific` is *worse* at 2.11 ns). What is
expensive is the **count** — ~14 resolutions per allocation, because each FFI
helper resolves independently and LLVM cannot CSE across the FFI boundary. The
inline bump removes the allocator's outright.

So the choice becomes per site rather than global. Loop membership is the
cheapest sound proxy for "this site runs many times" and bounds the size cost to
loop bodies. It reuses the existing `loop_targets` stack rather than adding a
counter: `switch` frames push an **empty** continue label while every loop
pushes a real one — the same discriminator `Stmt::Continue`'s
scan-outward-past-switch-frames logic already relies on — so a `new` inside a
bare `switch` is correctly treated as not-in-a-loop.

| bench | outlined | gate | unconditional-inline ceiling |
|---|--:|--:|--:|
| `churn_alloc` | 1.32 s | 0.73 s (**1.81×**) | 1.81× |
| `push_cls` | 1.30 s | 0.72 s (**1.81×**) | 1.78× |
| `churn` | 1.62 s | 1.04 s (**1.56×**) | 1.56× |
| `deeplist` | 1.66 s | 1.61 s (1.03×) | 1.03× |
| `retain` | 3.15 s | 3.06 s (1.03×) | 1.03× |
| `cycles` | 0.90 s | 0.90 s (1.00×) | 1.35× |
| `tree` | 8.97 s | 9.10 s (**0.986×**) | 1.05× |

The gate reaches the full unconditional-inline ceiling on the allocation-heavy
shapes. **`tree` costs 1.4%** (reproduced across two best-of-5 runs) — it
allocates in loops so it inlines, but its time is dominated by copying and
promotion rather than by the allocation call, so it pays the bloat without the
win. `cycles` gives up 0.35× against the ceiling because some of its `new` sites
sit outside loops.

Binary size, measured from both sides:

| program | outlined | gate | all-inline |
|---|--:|--:|--:|
| 800 sites, none in a loop | 13,150,096 | **13,150,096 (+0)** | +214,656 |
| 200 sites, none in a loop | 12,454,160 | **12,454,160 (+0)** | +49,536 |
| 800 sites, all in loops | 14,190,480 | +214,656 | +214,656 |

Zero growth for sites that are not in loops, and never worse than unconditional
inlining in the worst case.

`PERRY_INLINE_NEW=1` still forces the inline form everywhere for A/B work. Note
its test is `is_none()`, so `PERRY_INLINE_NEW=""` **enables** inlining rather
than disabling it — an empty string is `Some("")`.

Testing: 43 `test-files/` programs across both arms, identical output (the one
apparent diff was a `console.time` duration, which varies run-to-run on a single
binary), and identical under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` —
the arm that would catch a mis-written object header, since the inline bump
writes the header and zero-fills slots in generated code instead of in the
runtime.
