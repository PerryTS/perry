// #2021 — JSON.stringify over an array that has grown past its initial inline
// capacity (16) crashed with a Bus error. Array growth reallocates the
// header+inline-elements block and leaves a GC_FLAG_FORWARDED stub at the old
// address; element accessors resolve that forwarding (clean_arr_ptr, #233) but
// stringify_array_depth read length/elements straight off the stale stub.
// Fix: resolve forwarding at the stringify chokepoint. Output is byte-for-byte
// vs `node --experimental-strip-types`.

function build(n: number): Array<{ id: number; name: string; tags: string[] }> {
  const out: Array<{ id: number; name: string; tags: string[] }> = [];
  for (let i = 0; i < n; i++) {
    out.push({ id: i, name: "rec-" + i, tags: ["t" + i, "x"] });
  }
  return out;
}

// 16 stays inline (no growth); 17 forces the first reallocation; 40 forces a
// second. All three must stringify correctly, not just the small inline case.
console.log(JSON.stringify(build(16)).length);
console.log(JSON.stringify(build(17)));
console.log(JSON.stringify(build(40)).length);

// nested grown array inside an object field (object-field-array stringify path)
const wrapper = { items: build(20), count: 20 };
console.log(JSON.stringify(wrapper).length);

// array of primitives grown past 16 (non-object-shape path)
const nums: number[] = [];
for (let i = 0; i < 50; i++) nums.push(i * 3);
console.log(JSON.stringify(nums));
