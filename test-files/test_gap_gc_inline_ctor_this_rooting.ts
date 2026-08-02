// #7202: the inline-constructor path parks the instance in a plain
// `alloca_entry` slot (`this_slot`) that the collector never rewrites.
//
// `force_ctor_call` requires `class.constructor.is_some()`, so a class with
// FIELDS but no own constructor takes the inline path by default — no
// `PERRY_INLINE_CTOR` needed. Every `this` read inside the inlined body loads
// from that bare alloca. #7192 temp-roots the instance across the body, so an
// evacuating minor MOVES it rather than freeing it and rewrites the temp root —
// but `this_slot` still holds the pre-move address, so every field initializer
// after the collection stores into abandoned from-space memory and the fields
// simply never appear on the object the program keeps.
//
// `first`'s initializer allocates hard enough to reach the collector; `second`
// and `third` are stored AFTER it, through the stale `this`.
//
// LIVE BY CONSTRUCTION AND ONLY ON THE MOVING ARMS: a non-moving minor leaves
// the instance where it is, so `this_slot` stays accidentally correct.

function churn(): number {
  const a: any[] = [];
  for (let i = 0; i < 600; i++) {
    a.push({ i: i, s: "w" });
  }
  return a.length;
}

class Holder {
  first: number = churn();
  second: number = 42;
  third: string = "tail";
}

// `new Holder()` must ESCAPE, or scalar replacement deletes the object
// outright and every field becomes its own entry alloca — which #6968 already
// shadow-binds, so the inline-ctor `this_slot` is never materialized. Returning
// the instance from a separate function is the smallest escape that keeps the
// inline-constructor path live.
function mk(): any {
  return new Holder();
}

function run(): string {
  let badFirst = 0;
  let badSecond = 0;
  let badThird = 0;
  for (let r = 0; r < 400; r++) {
    const h = mk();
    if (h.first !== 600) badFirst++;
    if (h.second !== 42) badSecond++;
    if (h.third !== "tail") badThird++;
  }
  return "first " + badFirst + " second " + badSecond + " third " + badThird;
}

console.log("bad", run());
