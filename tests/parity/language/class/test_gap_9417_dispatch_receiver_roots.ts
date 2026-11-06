// #9417: the dynamic instance-method dispatch tower held the RECEIVER in a bare
// SSA register across the lowering of every ARGUMENT expression.
//
// `lower_call/property_get/dynamic_dispatch.rs` lowered `object` first (JS
// evaluation order requires that), then lowered each argument — arbitrary user
// code that allocates — and only then consumed the receiver in
// `js_object_get_own_field_or_undef` / `js_native_call_method` /
// `js_implicit_this_set`. An evacuating young-gen minor inside an argument moves
// the receiver, and the register keeps the pre-collection address.
//
// Nothing faults at the move: `js_object_get_own_field_or_undef` fails its
// `obj_type == GC_TYPE_OBJECT` check on the recycled cell and answers
// TAG_UNDEFINED, so the own-override probe misses and the by-name dispatch runs
// on a retired address. The observable is a wrong answer several steps
// downstream — which is why #9417 presented as `reading 'def'` on a property
// nowhere near the defect.
//
// TWO THINGS THIS FIXTURE NEEDS, AND BOTH ARE LOAD-BEARING:
//
// 1. The receiver must NOT be a plain local read. A load out of a shadow slot is
//    a re-readable location, so `root_reload.rs` re-derives it below the
//    collection point and the shape is already correct. It has to be a value
//    with no such location — a call result — which is exactly cc's site
//    (`js_native_call_value` -> spill -> object-literal ctor -> reload).
// 2. The argument must allocate past the 16 MiB nursery cap
//    (`SCAVENGE_NURSERY_CAP_DEFAULT_MB`) with cells that ESCAPE. A
//    scalar-replaced object literal never reaches the arena, and a churn that
//    allocates nothing is a test that cannot fail.

class Tagged {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  join(other: string): string {
    return this.tag + "|" + other;
  }
}

// A second implementor of `join` keeps the call on the dispatch tower rather
// than a single static callee.
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
// takes the dynamic instance-method dispatch path.
function makeTagged(k: number): any {
  return new Tagged("t" + k);
}

function churn(n: number): string {
  let out = "x";
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    const cell = { a: i, b: i + 1, c: i + 2, d: i + 3 };
    keep.push(cell);
    if (keep.length >= 1024) {
      out = "" + keep[0].c;
      keep = [];
    }
  }
  return out;
}

function main(): void {
  // THE GAP. The receiver is the result of `makeTagged(k)`; the argument
  // `churn(...)` collects while it is live only in a register.
  let bad = 0;
  let firstKind = "";
  for (let k = 0; k < 8; k++) {
    const expect = "t" + k + "|";
    let got: any = "";
    try {
      got = makeTagged(k).join(churn(500000));
    } catch (e) {
      got = "threw";
    }
    const ok = typeof got === "string" && got.indexOf(expect) === 0;
    if (!ok) {
      bad++;
      if (firstKind === "") {
        firstKind = typeof got === "string" ? "wrong-string" : typeof got;
      }
    }
  }
  console.log("dispatch-receiver bad=" + bad);
  console.log("first bad kind=" + firstKind);

  // CONTRACT, NOT A GAP: the same dispatch with a non-collecting argument must
  // keep working. This half has never failed — it is here so a fix that
  // over-roots or mis-orders the group is caught, not because it demonstrates
  // the defect.
  const keep: any = new OtherTagged("x");
  console.log("control=" + keep.join("y"));
}

main();
