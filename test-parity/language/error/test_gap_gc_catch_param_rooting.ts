// #7209: the `catch (e)` parameter has a shadow slot RESERVED for it and never
// BOUND, so the exception object has no root at all for the whole catch body.
//
// `collectors/pointer_locals.rs` assigns the catch parameter a slot index (it
// is implicitly `Any`, i.e. pointer-possible), so `js_shadow_frame_enter`'s
// count already includes it. `stmt/try_stmt.rs` then allocates the parameter's
// storage with `alloca_entry` and stores the exception into it — and never
// emits `js_shadow_slot_bind`, so `active[idx]` stays false forever and the
// collector never dereferences the alloca. The frame is sized for a root that
// does not exist.
//
// What makes it sharp rather than merely untidy: `js_clear_exception()` runs
// BEFORE the catch body is lowered, dropping the runtime's own reference. From
// that point the only thing referring to the exception is the unrooted alloca.
// Under a precise-roots collection the object can be SWEPT, not merely moved.
//
// LIVE BY CONSTRUCTION. `churn()` allocates hard enough to reach the collector,
// and `e` is read AFTER it — both the message string and a field stored on the
// error, so a relocated `Error` and a reclaimed one both show up.

function churn(): number {
  const a: any[] = [];
  for (let i = 0; i < 600; i++) {
    a.push({ i: i, s: "e" });
  }
  return a.length;
}

function run(): string {
  let badChurn = 0;
  let badMessage = 0;
  let badField = 0;
  for (let r = 0; r < 400; r++) {
    try {
      const err: any = new Error("boom" + r);
      err.tag = r;
      throw err;
    } catch (e: any) {
      // The exception is live across this call and nothing else refers to it.
      const n = churn();
      if (n !== 600) badChurn++;
      if (e.message !== "boom" + r) badMessage++;
      if (e.tag !== r) badField++;
    }
  }
  return "churn " + badChurn + " message " + badMessage + " field " + badField;
}

console.log("bad", run());
