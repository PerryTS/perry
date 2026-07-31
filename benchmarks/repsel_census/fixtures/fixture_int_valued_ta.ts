// Liveness fixture for the `int-valued-ta` census key (#7106).
//
// `collectors/int_valued_ta_locals.rs` admits a local as native-i32 even when
// one of its writes is a POSSIBLY-OUT-OF-BOUNDS int typed-array read — the
// bcryptjs `_encipher` Feistel-accumulator shape the collector was written
// for. Soundness rests on a whole-function observation constraint, so the
// fixture reproduces it exactly:
//
//   rule 1 (writes)       — every write is an int-TA element read, a bitwise
//                           op, or `Math.imul`; never `*`, never a plain-array
//                           read, never a float/Uint32 TA
//   rule 2 (observations) — every use is a direct bitwise operand or a store
//                           into an int-kind typed array; never an array index,
//                           an additive operand, a comparison, a call argument,
//                           a `return`, or a `console.log`
//
// `off` is a parameter, so `lr[off]` is NOT provably in bounds — that is the
// whole point. Adding `if (off < lr.length)` would move these locals to the
// ordinary `integer_locals` path and take this fixture to zero.

function encipher(lr: Int32Array, off: number, P: Int32Array): void {
  let l = lr[off];
  let r = lr[off + 1];
  l = l ^ P[0];
  r = r ^ l;
  r = r ^ P[1];
  l = l ^ Math.imul(r, 3);
  l = l ^ (r >>> 8);
  r = r ^ (l << 3);
  lr[off] = r;
  lr[off + 1] = l;
}

const state = new Int32Array(8);
const box = new Int32Array(4);
state[0] = 11;
state[1] = 22;
box[0] = 0x243f6a88;
box[1] = 0x85a308d3;
encipher(state, 0, box);
console.log("int_valued_ta:" + state[0]);
