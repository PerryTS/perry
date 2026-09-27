// Property-read control: the bare loop PLUS one static-key read of a plain
// object's field per iteration (`box.n` on an object returned from a
// function) — the shape every workload's loop bound had before the bounds
// were hoisted into locals. In Perry 0.5.1654 this read takes the generic
// by-name path (js_object_get_field_by_name_f64 -> inherited_read_cache_lookup),
// ~1k instructions; likely the same root cause as #11420. It is a Perry
// finding, NOT per-workload overhead: nothing is subtracted using it.
import { iters, header, hex, mulFnv } from "../_lib/bench.ts";

const it = iters(1000000, 1000);
header("control/prop_read", "", it);

function makeBox(v: number): { n: number } {
  return { n: v };
}
const box = makeBox(7);

function op(h: number, i: number): number {
  return mulFnv((h ^ i ^ box.n) >>> 0);
}

const N = it.n, WARM = it.warm;
let h = 2166136261;
for (let i = 0; i < WARM; i++) h = op(h, i);
h = 2166136261;
for (let i = 0; i < N; i++) h = op(h, i);
console.log("checksum " + hex(h));
