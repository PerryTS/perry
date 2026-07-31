// Liveness fixture for the `spec-abi-entry` and `spec-abi-taptr-slot` census
// keys (#7106).
//
// `TaPtr` is one of the six unboxed representations, but unlike the others it
// is a *parameter* rep rather than a local rep: it lives inside a specialized
// entry's rep tuple (`SpecParamRep::label()` spells it `ta<kind>` /
// `ta<kind>x<len>`). The census counts those slots by parsing the tuple, so
// this fixture keeps both keys honest.
//
// What `collectors/spec_abi_sites.rs` needs:
//   - a module binding whose SINGLE top-level `let`/`const` init is
//     `new Int32Array(<integer literal>)` — a non-view construction form
//   - that binding never reassigned anywhere in the module and never
//     referenced inside a closure body
//   - a DIRECT call to a user function passing it, in a LATER top-level
//     statement than the binding (structural dominance)
//   - integer-literal / numeric-literal args for the I32 / F64 slots
//
// Reassigning `TABLE`, or reading it inside an arrow function, drops the
// judgment to `Boxed` and takes both keys to zero.

const TABLE = new Int32Array(256);
const SCRATCH = new Int32Array(16);

function mix(table: Int32Array, scratch: Int32Array, seed: number, scale: number): number {
  let acc = seed | 0;
  for (let i = 0; i < 16; i++) {
    acc = acc ^ table[i & 255];
    scratch[i & 15] = acc;
  }
  return acc * scale;
}

for (let i = 0; i < 256; i++) {
  TABLE[i] = i * 7;
}

console.log("spec_abi_taptr:" + mix(TABLE, SCRATCH, 17, 1.5));
