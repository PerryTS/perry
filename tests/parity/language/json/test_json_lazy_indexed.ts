// Issue #179 Step 2 Phase 3: exercises indexed/property access on
// JSON.parse results. Must match Node byte-for-byte under BOTH the
// direct parser (default) AND the lazy parser (PERRY_JSON_TAPE=1),
// which means the lazy path's force-materialize contract in
// clean_arr_ptr + the runtime obj_type guard on the inline IndexGet
// codegen both have to correctly route through js_array_get_f64 +
// materialize on first index/field access.

interface Record {
  id: number;
  name: string;
  score: number;
  nested: { a: number; b: number };
}

const blob = '[{"id":1,"name":"alpha","score":9.5,"nested":{"a":10,"b":20}},{"id":2,"name":"beta","score":8.25,"nested":{"a":30,"b":40}},{"id":3,"name":"gamma","score":7.75,"nested":{"a":50,"b":60}}]';

const parsed = JSON.parse(blob) as Record[];

// .length first (lazy fast path)
console.log("len:" + parsed.length);

// Indexed access — forces materialize under lazy flag via
// clean_arr_ptr's obj_type check, then the runtime
// obj_type-guard in the inline IndexGet codegen routes through
// js_array_get_f64 which also hits the materialize path.
console.log("0.id:" + parsed[0].id);
console.log("1.name:" + parsed[1].name);
console.log("2.score:" + parsed[2].score);
console.log("1.nested.a:" + parsed[1].nested.a);
console.log("2.nested.b:" + parsed[2].nested.b);

// for-loop iteration — exercises the bounded-index fast path's
// lazy guard on every iteration.
let sum = 0;
for (let i = 0; i < parsed.length; i++) {
  sum += parsed[i].id + parsed[i].nested.a + parsed[i].nested.b;
}
console.log("sum:" + sum);

// Length after indexed access — must still match (materialized
// tree's .length == cached_length from before).
console.log("len-after:" + parsed.length);

// Stringify AFTER indexed access. Materialized != null so the
// lazy stringify fast path (memcpy) is skipped and the generic
// tree walk runs — produces the same bytes as the direct path.
const roundtrip = JSON.stringify(parsed);
console.log("roundtrip-len:" + roundtrip.length);
