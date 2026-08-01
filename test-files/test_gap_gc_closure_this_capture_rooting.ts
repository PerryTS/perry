// #7208: a closure's captured `this` lives in a plain `alloca_entry` that no
// `js_shadow_slot_bind` names, so the collector neither marks nor rewrites it.
//
// `codegen/closure.rs` sized its shadow frame `enable_shadow_frame(m.len())`
// where `codegen/method.rs` uses `m.len() + 1` — and that `+1` IS the `this`
// slot, which `method.rs` then binds. Closures reserved nothing, so there was
// no index to bind and the receiver was unrooted for the whole body.
//
// The in-tree comment claiming the capture reads are "exempt ... they run in
// the entry-block prologue, ahead of any statement that could collect"
// justifies the timing of the READ. It says nothing about the lifetime of the
// SLOT, which spans every statement in the body — including the ones that
// collect.
//
// LIVE BY CONSTRUCTION. The receiver is a temporary: after `make()` returns it
// is reachable ONLY from the closure's capture cell, which IS a traced root —
// so an evacuating minor MOVES it rather than freeing it, rewrites the capture
// cell, and leaves the prologue's alloca copy naming from-space. `churn()` runs
// between the prologue read and the `this.tag` read.

function churn(): number {
  const a: any[] = [];
  for (let i = 0; i < 600; i++) {
    a.push({ i: i, s: "c" });
  }
  return a.length;
}

class Holder {
  tag: number;
  label: string;
  constructor(t: number) {
    this.tag = t;
    this.label = "h";
  }
  make(): () => string {
    return () => {
      const n = churn();
      return this.label + ":" + (this.tag + (n - 600));
    };
  }
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 400; r++) {
    const f = new Holder(r).make();
    if (f() !== "h:" + r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
