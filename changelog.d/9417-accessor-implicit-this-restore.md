**An accessor call no longer corrupts the caller's `this` across an evacuating
young-gen minor** (#9417) — the defect behind claude-code answering
`Cannot read properties of undefined (reading 'def')` where node says
`Not logged in · Please run /login`.

`invoke_accessor_getter` / `invoke_accessor_setter`
(`perry-runtime/src/object/field_get_set/accessors.rs`) bind an accessor's
receiver by writing the GC-rooted `IMPLICIT_THIS` cell and keeping the previous
occupant in a **bare Rust local** for the duration of the accessor body:

```rust
let prev = js_implicit_this_set(eff_receiver);
let result = js_closure_call0(closure);   // USER CODE — allocates
js_implicit_this_set(prev);               // pre-collection address
```

The body is user code, so it allocates; a copying minor there relocates the
caller's receiver and rewrites every slot it can see — and a Rust local is not
one (#7249 / #7498). The restore then reinstalled a **retired from-space
address** as the caller's `this`. Two further locals in the same two functions
had the same shape: `get_bits`/`set_bits` across `coerce_call_this`'s primitive
boxing, and the receiver plus the setter's assigned `value` across
`clone_closure_rebind_this`'s fresh `ClosureHeader` allocation. All are now
rooted in a `RuntimeHandleScope` and re-read at their point of use.

**Nothing crashed, which is why nothing caught it.** A property read off the
retired cell reaches `js_object_get_own_field_or_undef`, which fails its
`obj_type == GC_TYPE_OBJECT` check and returns `TAG_UNDEFINED` rather than
faulting — so `this.<field>` silently answers `undefined` and the *next* member
access throws a TypeError naming a property several steps downstream of the
real defect.

**How it was found.** `PERRY_GC_MOVING_LOOP_POLLS=0` and a large
`PERRY_GC_SCAVENGE_NURSERY_MB` both made the claude-code divergence vanish,
placing it on the evacuating minor; `PERRY_GC_PROTECT_FROMSPACE=1` then faulted
on the exact stale use, and the backtrace off a `PERRY_KEEP_SYMBOLS=1` build
read `js_object_get_own_field_or_undef` ← *(JS getter frames)* ←
`invoke_accessor_getter` ← `builtin_reflection_accessor_read` ←
`js_object_get_field_ic_miss`.

**Test.** `test-files/test_gap_9417_accessor_this_restore.ts` — an accessor
whose body allocates, called from a method that reads `this` afterwards. On
unfixed `main` it prints `caller-this bad=30` with claude-code's exact message,
deterministically and with no GC env knobs; it is now byte-identical to
`node --experimental-strip-types`.

**Not fixed here:** the same unrooted `let prev = js_implicit_this_set(x); …;
js_implicit_this_set(prev)` shape appears at ~18 other runtime sites (timers,
node streams, dgram, event_target, `Map`/`Set` `forEach`, promisify,
os_process_streams). `iterator_helpers.rs` is the one site that already roots
the saved value; the rest are the same latent hazard and want a follow-up
sweep.
