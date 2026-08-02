// #7200: `{ ...src, tail: 7 }` where `src` carries an ACCESSOR must survive an
// evacuating minor that runs inside the spread helper.
//
// Since #809 an object literal containing a spread lowers to a source-ordered
// IIFE built on `js_object_assign_one` (`lower/expr_object.rs`), NOT to
// `Expr::ObjectSpread` (which is JSX-only). `js_object_assign_one` reads every
// own key of the source, so a getter there runs arbitrary USER CODE inside the
// runtime helper — and user code reaches a loop back-edge poll, which under
// `PERRY_GC_MOVING_LOOP_POLLS=1` is an evacuating minor.
//
// Two values are stale across that window and both had to be fixed:
//   * codegen side — the accumulator `acc` was threaded through a bare SSA
//     register across every `js_object_assign_one` call, so the destination
//     object named from-space after the first accessor collected;
//   * runtime side — `js_object_assign_one` held the target, the source and the
//     key list in Rust locals across the getter invocation and stored into them
//     afterwards.
//
// LIVE BY CONSTRUCTION AND ONLY ON THE MOVING ARMS. The getter allocates hard
// enough to reach the collector, and the copied value is read back immediately
// after — a non-moving collection cannot expose it, so the `requires=move` arms
// are the ones that bite.

function churn(): number {
  const a: any[] = [];
  for (let i = 0; i < 600; i++) {
    a.push({ i: i, s: "z" });
  }
  return a.length;
}

function run(): string {
  let badPlain = 0;
  let badHot = 0;
  let badTail = 0;
  for (let r = 0; r < 400; r++) {
    const src: any = { plain: 5 };
    Object.defineProperty(src, "hot", {
      enumerable: true,
      get: function () {
        return churn();
      },
    });
    const out: any = { ...src, tail: 7 };
    if (out.plain !== 5) badPlain++;
    if (out.hot !== 600) badHot++;
    if (out.tail !== 7) badTail++;
  }
  return "plain " + badPlain + " hot " + badHot + " tail " + badTail;
}

console.log("bad", run());
