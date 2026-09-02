// #9523: `Expr::SetHas` and `Expr::SetDelete` unboxed the receiver to a raw
// `i64` handle BEFORE lowering the value expression and consumed that handle
// after it — the #6970 shape their Map twins (`MapGet` / `MapHas` /
// `MapDelete`) were fixed for.
//
// `crates/perry-codegen/src/expr/bigint_set.rs` lowered `set`, masked the
// pointer out of the NaN-box into an SSA register, THEN lowered `value` —
// arbitrary user code that allocates — and only then called `js_set_has` /
// `js_set_delete` with the register. An evacuating young-gen minor inside the
// value's evaluation moves the Set; the register keeps the pre-move address,
// and the runtime helper reads a from-space header. Nothing faults at the
// move: the answer is simply wrong (`has` false for a member, `delete` false
// and the member still present) or the process dies on a recycled cell.
//
// TWO THINGS THIS FIXTURE NEEDS, AND BOTH ARE LOAD-BEARING:
//
// 1. The receiver must be a MODULE-LEVEL binding read from inside a function.
//    A function-local Set lives in a shadow slot, and `root_reload.rs` already
//    re-materialises a slot load — together with the `bitcast`/`and` unmask
//    derived from it — below every collection point that can reach a use, so
//    a plain local receiver passes on the unfixed compiler. A module-level
//    binding is a `@perry_global_*` load, which that pass deliberately does
//    NOT reload ("that population needs rooting, not reloading"), so the raw
//    handle is the only copy the consuming call ever sees.
// 2. The value must allocate past the 16 MiB nursery cap
//    (`SCAVENGE_NURSERY_CAP_DEFAULT_MB`) with cells that ESCAPE, the
//    `test_gap_9417_dispatch_receiver_roots.ts` recipe. A scalar-replaced
//    object literal never reaches the arena, and a churn that allocates
//    nothing is a test that cannot fail.
//
// Each probe allocates a FRESH Set immediately before the call, so the Set is
// a nursery object when the value's churn runs and the minor that fires there
// is the one that evacuates it.

let stringSet: Set<string> = new Set<string>();
let numberSet: Set<number> = new Set<number>();
let anySet: Set<any> = new Set<any>();

function churn(n: number): void {
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    const cell = { a: i, b: i + 1, c: i + 2, d: i + 3 };
    keep.push(cell);
    if (keep.length >= 1024) {
      keep = [];
    }
  }
}

// The value expressions. Each one collects (churn) and then builds the key it
// returns, so the key itself is a post-collection object.
function stringKey(k: number): string {
  churn(400000);
  return "k" + k;
}
function numberKey(k: number): number {
  churn(400000);
  return k * 3 + 0.5;
}
function anyKey(k: number): any {
  churn(400000);
  return "a" + k;
}

// THE GAP, string-typed receiver (`js_set_has_string` / `js_set_delete_string`).
function probeStringSet(k: number): string {
  stringSet = new Set<string>();
  stringSet.add("k" + k);
  const has = stringSet.has(stringKey(k));
  stringSet = new Set<string>();
  stringSet.add("k" + k);
  const del = stringSet.delete(stringKey(k));
  return has + "/" + del + "/" + stringSet.size;
}

// THE GAP, number-typed receiver (the guarded number arm).
function probeNumberSet(k: number): string {
  numberSet = new Set<number>();
  numberSet.add(k * 3 + 0.5);
  const has = numberSet.has(numberKey(k));
  numberSet = new Set<number>();
  numberSet.add(k * 3 + 0.5);
  const del = numberSet.delete(numberKey(k));
  return has + "/" + del + "/" + numberSet.size;
}

// THE GAP, untyped value (the generic `js_set_has` / `js_set_delete` arm).
function probeAnySet(k: number): string {
  anySet = new Set<any>();
  anySet.add("a" + k);
  const has = anySet.has(anyKey(k));
  anySet = new Set<any>();
  anySet.add("a" + k);
  const del = anySet.delete(anyKey(k));
  return has + "/" + del + "/" + anySet.size;
}

// CONTRACT, NOT A GAP: a value that cannot collect leaves no window, so the
// lowering must stay on its unprotected path and still answer correctly. Here
// so a fix that over-roots or mis-orders the group is caught.
function probeControl(k: number): string {
  stringSet = new Set<string>();
  const key = "k" + k;
  stringSet.add(key);
  const has = stringSet.has(key);
  const del = stringSet.delete(key);
  return has + "/" + del + "/" + stringSet.size;
}

function main(): void {
  const rounds = 6;
  let bad = 0;
  let first = "";
  for (let k = 0; k < rounds; k++) {
    const results = [
      "string " + probeStringSet(k),
      "number " + probeNumberSet(k),
      "any " + probeAnySet(k),
    ];
    for (const r of results) {
      if (r.indexOf(" true/true/0") < 0) {
        bad++;
        if (first === "") {
          first = r;
        }
      }
    }
  }
  console.log("set-receiver bad=" + bad);
  console.log("first bad=" + first);
  console.log("control=" + probeControl(0));
}

main();
