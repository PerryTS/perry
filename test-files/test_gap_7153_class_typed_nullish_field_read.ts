// #7153: a field read through a CLASS-ANNOTATED binding whose runtime value is
// nullish must throw TypeError, exactly like Node — not answer `undefined`.
//
// The bug: `const missing = short[5]` on a `Row[]` refines the binding to
// `Named("Row")`, so the read takes the class-field guard diamond
// (property_get.rs / property_get/helpers.rs). The guard correctly rejects a
// non-pointer receiver, but the fallback funneled straight into
// `js_object_get_field_by_name_f64`, which answers `undefined` for any
// unrecognized receiver — the program kept running on a silent wrong value.
// An `any`-typed binding took the generic dispatch path, which has thrown
// correctly since #462. The reads below are DIRECT (no closure wrapper — a
// capture could reroute the lowering) and every case prints the caught
// error's name and message so the file is byte-exact against Node.

class Row {
  id: number;
  name: string;
  score: number;
  constructor(id: number, name: string, score: number) {
    this.id = id;
    this.name = name;
    this.score = score;
  }
}

const short: Row[] = [];
short.push(new Row(9, "s", 9));

// 1. Value context: the plain class-field guard diamond.
const missing = short[5];
try {
  console.log("oob value:", missing.id);
} catch (e) {
  console.log("oob value:", (e as Error).constructor.name + ": " + (e as Error).message);
}

// 2. Number context: the raw-f64 number-context variant of the same diamond
//    (`missing.score * 2` routes through the ToNumber-flavored lowering).
try {
  console.log("oob number:", missing.score * 2);
} catch (e) {
  console.log("oob number:", (e as Error).constructor.name + ": " + (e as Error).message);
}

// 3. String-typed field on the same nullish receiver.
try {
  console.log("oob string field:", missing.name);
} catch (e) {
  console.log("oob string field:", (e as Error).constructor.name + ": " + (e as Error).message);
}

// 4. A null element behind the same class annotation: the message must say
//    "null", not "undefined".
const holed: Row[] = [];
holed.push(null as unknown as Row);
const nullish = holed[0];
try {
  console.log("null value:", nullish.id);
} catch (e) {
  console.log("null value:", (e as Error).constructor.name + ": " + (e as Error).message);
}
try {
  console.log("null number:", nullish.score + 1);
} catch (e) {
  console.log("null number:", (e as Error).constructor.name + ": " + (e as Error).message);
}

// 5. Control: an in-bounds element still reads normally through the same
//    lowering (the fix must not disturb the fast path or valid fallbacks).
const present = short[0];
console.log("in bounds:", present.id, present.name, present.score * 2);
