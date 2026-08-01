// #7201: a class EXPRESSION carrying a `static { … }` block whose body
// allocates must keep working under an evacuating minor.
//
// #7192/#7198 rooted the class object itself: `Expr::ClassExprFresh` now
// temp-roots it and re-reads that root before every use, including before
// `js_static_this_arm_value` and after the static-block body returns. The crash
// survived that, because the stale value is the one the RUNTIME parks: the
// static-`this` one-shot cell that `js_static_this_arm_value` writes and the
// compiled block body reads back through `js_static_this_resolve`. That cell
// was a plain thread-local word — not marked, and above all not REWRITTEN on
// evacuation — so if the class object relocates between the arm and the
// resolve, or between the resolve and the body's `this.x = …` stores, the cell
// hands out a from-space address.
//
// LIVE BY CONSTRUCTION AND ONLY ON THE MOVING ARMS. The block body allocates
// past the nursery, and both statics are read back immediately after the
// factory returns.

function churn(): number {
  const a: any[] = [];
  for (let i = 0; i < 600; i++) {
    a.push({ i: i, s: "q" });
  }
  return a.length;
}

function make(): any {
  return class {
    static k: number = 1;
    static {
      (this as any).viaBlock = churn();
    }
  };
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 400; r++) {
    const C: any = make();
    if (C.k !== 1) {
      bad++;
    }
    if (C.viaBlock !== 600) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
