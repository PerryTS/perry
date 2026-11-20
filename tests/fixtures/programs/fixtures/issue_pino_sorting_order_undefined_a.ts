// First half of the #901 regression repro. Pair file for
// `test_issue_pino_sorting_order_undefined.ts`. Both this file and
// `_b.ts` export an object via `export default` — the consumer
// reads a per-module property on each to verify the two defaults
// don't collide.
const a = { value: "AAA", tag: "constants-shape" };
export default a;
