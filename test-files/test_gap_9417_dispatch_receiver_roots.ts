// #9417: the dynamic instance-method dispatch tower lowered the RECEIVER into a
// bare SSA register, then lowered every ARGUMENT expression — arbitrary user
// code that allocates — and only then consumed the receiver. An evacuating
// young-gen minor inside the argument moves the receiver and the register keeps
// the pre-collection address.
//
// Nothing faults at the move: `js_object_get_own_field_or_undef` fails its
// `obj_type == GC_TYPE_OBJECT` check on the recycled cell and answers
// TAG_UNDEFINED, so the override probe misses and the by-name dispatch runs on a
// retired address. The observable is a wrong answer, several steps downstream.
//
// The receiver must NOT be a plain local read: a load out of a shadow slot is
// re-derived by `root_reload.rs`. It has to be a value with no re-readable
// location — a call result — which is exactly what cc's site is
// (`js_native_call_value` -> spill -> object-literal ctor -> reload).

class Tagged {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  join(other: string): string {
    return this.tag + "|" + other;
  }
}

// A second implementor keeps the dispatch tower from collapsing to a single
// static callee, which is what `needs_dynamic_dispatch` selects on.
class OtherTagged {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  join(other: string): string {
    return "other|" + other;
  }
}

// Returns `any`, so the receiver's class is not statically known and the call
// goes through the dynamic instance-method dispatch tower.
function makeTagged(k: number): any {
  return new Tagged("t" + k);
}

// Allocates enough to drive at least one evacuating young-gen minor while the
// receiver is live only in a register.
function churn(n: number): string {
  let out = "";
  for (let i = 0; i < n; i++) {
    const cell = { a: i, b: i + 1, c: i + 2 };
    if (cell.a === n - 1) {
      out = "" + cell.c;
    }
  }
  return out;
}

function main(): void {
  let bad = 0;
  let firstBad = "";
  for (let k = 0; k < 60; k++) {
    const expect = "t" + k + "|";
    let got = "";
    try {
      got = makeTagged(k).join(churn(4000));
    } catch (e) {
      got = "threw:" + (e as Error).message;
    }
    if (got.indexOf(expect) !== 0) {
      bad++;
      if (firstBad === "") {
        firstBad = got;
      }
    }
  }
  // Keep `OtherTagged` reachable so it is registered as an implementor.
  const keep: any = new OtherTagged("x");
  if (keep.join("y") !== "other|y") {
    console.log("control broken");
  }
  console.log("dispatch-receiver bad=" + bad);
  if (bad > 0) {
    console.log("first bad: " + firstBad);
  }
}

main();
