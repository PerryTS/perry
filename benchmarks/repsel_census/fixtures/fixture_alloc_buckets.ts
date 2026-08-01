// Liveness fixture for the `Ptr<Shape>` ALLOC-SITE BUCKETS (#7170 R0).
//
// The other `ptr_shape` fixtures assert that a promotion can be observed. This
// one asserts something the census could not see at all before R0: that the
// three *denial* buckets the report files unbound allocations into are still
// being told apart.
//
// They were not. #7170 measured two merges, each of which made a
// scheduler-facing number mean something other than what it looked like:
//
//   * `constructor argument` conflated a genuine `new C(arg)` (2.0% of sites
//     on the dependency corpus) with the property values of anonymous-shape
//     object literals (24.5%) — because `{a: {b: 1}}` lowers to
//     `new __AnonShape_N(<inner literal>)`, so the inner literal really is an
//     argument of a `new`, just not of a constructor anybody wrote.
//   * the `return` bucket counted syntactic sites rather than opportunities:
//     `collectors/ptr_shape_returns.rs` (#7107) already admits a bare
//     `return new C(...)` as a return-shape producer, but `deny_alloc_site`
//     fires before any seeding and could not know.
//
// Each of the three shapes below lands in exactly one bucket, and
// `ALLOC_BUCKET_FLOORS` / `ALLOC_RULE_FLOORS` require all three to be
// reported. A classification arm that stops firing re-merges its bucket into a
// neighbour, the count for that bucket goes to zero, and the census goes red —
// instead of the merge being invisible, which is the state R0 found.
//
// This is also the only layer that exercises the WIRING. The unit tests set the
// report scope by hand, so `codegen/function.rs` could pass `false` for every
// function's return-shape fact and every one of them would still pass. Here the
// fact has to travel from `collect_return_shape_functions` through
// `ModuleDispatchFacts` into the region scope for `makePoint`'s row to carry
// the served rule.
//
// Do not "tidy" this file:
//   * giving `makePoint` a second return of a different class, or an early
//     `return;`, revokes its return-shape fact and takes the served bucket to
//     zero;
//   * binding the inner literal of `nested` to its own `const` moves it out of
//     the anonymous-shape-component bucket entirely (it becomes rule 1's own
//     seed);
//   * adding `Object.defineProperty` / `delete` / `setPrototypeOf` / `Proxy`
//     anywhere arms the rule-5 module barrier, which kills every return-shape
//     fact in the module and silently empties the served bucket.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Boxed {
  v: Point;
  constructor(v: Point) {
    this.v = v;
  }
}

// Bucket 1 — `return`, in a function that DOES carry a return-shape fact:
// every return is a bare fresh `new` of one declared, admissible class, the
// body cannot fall off its end, and the declared return type is a pointer.
// So this row must be filed under the served rule, not the rule-1 wall.
export function makePoint(i: number): Point {
  return new Point(i, i + 1);
}

// Bucket 2 — a genuine `new C(arg)` constructor argument. `Boxed` is a class
// the developer declared, so `new Point(1, 2)` here is what the label says.
const boxed = new Boxed(new Point(1, 2));

// Bucket 3 — an anonymous-shape component. `{ inner: ..., k: 3 }` is a closed
// shape, so it lowers to `new __AnonShape_N(<inner literal>, 3)` and the inner
// literal arrives as an argument of that synthetic constructor. It is a field
// value of a parent allocation that is itself unbound: proving its shape
// licenses nothing on its own, which is exactly why it must not share a bucket
// with the line above.
const nested = { inner: { a: 1, b: 2 }, k: 3 };

const p = makePoint(3);
console.log(
  "alloc_buckets:" + (boxed.v.x + nested.inner.a + nested.k + p.x + p.y),
);
