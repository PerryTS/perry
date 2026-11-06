// Second half of the #901 regression repro. Pair file for
// `test_issue_pino_sorting_order_undefined.ts`. Both this file and
// `_a.ts` export an object via `export default` — the consumer
// reads a per-module property on each to verify the two defaults
// don't collide.
const b = { value: "BBB", tag: "tools-shape" };
export default b;
