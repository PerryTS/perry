### Fixed

- **A relocating young collection no longer invalidates a closure's own `this_closure` pointer** (#7055). A closure body reaches its captures through `js_closure_get_capture_bits(%this_closure, idx)`, and `%this_closure` is an LLVM parameter — a register value no root enumeration can see. The shipped default runs an evacuating young collection at loop back-edge polls (`js_gc_loop_safepoint`) with precise roots and no conservative native-stack scan, so a closure relocated while its own body was on the stack left that register pointing into from-space; the same cycle resets from-space and hands it straight back to the mutator, after which `js_closure_get_capture_bits` read a foreign object's `capture_count`, judged the index out of range, and returned **0**. Pointer 0 is not a registered box, so every later boxed-capture read yielded `undefined` and every boxed-capture write was silently dropped.

  In an `async fn` the casualty was the generator state machine itself: the async-to-generator transform boxes every body local, `__gen_state` included, so the `state = <next>` store at the end of a resumed state body went nowhere and the following `await` resumed into the state it had just finished — **replaying one loop iteration** and folding its contribution into the accumulator twice. That is the shape #7055 reported on an ordinary event-loop request pump: a deterministic checksum error of *fixed* magnitude at 150 / 300 / 600 / 1200 requests, present only when `copied_objects > 0` and absent under `PERRY_GEN_GC=0`.

  Closure bodies with captures now spill `%this_closure` into a NaN-boxed entry alloca bound to a shadow-stack slot (`reserve_shadow_slot` + `js_shadow_slot_bind`), and `expr::current_closure_ptr_value` reloads the pointer from that slot at every capture access — so the collector marks and rewrites it like any other precise root. Capture-less closures emit no capture access and keep their frame-free prologue, so `(a, b) => a - b` costs nothing. The typed-ABI clone is untouched and is not a hole: `lower_typed_f64_body_*` bails on anything that is not straight-line arithmetic, so a typed body contains no poll and no allocation. No runtime change — `crates/perry-runtime` is byte-identical.

  Minimal reproducer (11 lines; `calls` must be 3, printed 4 under any relocating arm):

  ```ts
  let calls = 0;
  async function main(): Promise<void> {
    for (let r = 0; r < 3; r++) {
      calls = calls + 1;
      let o: any = null;
      for (let i = 0; i < 50; i++) { o = { id: i }; }
      await Promise.resolve();
    }
    console.log("calls:" + calls);
  }
  main();
  ```

  The issue's `w5_srv_scale.ts` is now byte-exact against node 26.5.0 at 150 / 300 / 600 requests in the shipped default, and across `PERRY_GC_SCAVENGE_NURSERY_MB` 1–16, `PERRY_GC_MOVING_SAFEPOINT=0`, `PERRY_GC_SCAVENGE=1`, `PERRY_GC_FORCE_EVACUATE=1` and `PERRY_GEN_GC=0`. Covered by a `cargo-test`-visible codegen IR-shape test and `crates/perry/tests/gc_closure_self_pointer_root_7055.rs`, both sabotage-verified in both directions.
