// Behavioral parity test for the nanoid package (perry-stdlib).
//
// nanoid output is non-deterministic — assert length, alphabet, and
// distinctness invariants instead of concrete values.

import { nanoid, customAlphabet } from "nanoid";

// ── Default size (21) ──
const id = nanoid();
console.log("default length:", id.length);
console.log("default is string:", typeof id === "string");

const URL_SAFE = /^[A-Za-z0-9_-]+$/;
console.log("default url-safe:", URL_SAFE.test(id));

// ── Custom size ──
const small = nanoid(8);
console.log("size 8 length:", small.length);
console.log("size 8 url-safe:", URL_SAFE.test(small));

const big = nanoid(32);
console.log("size 32 length:", big.length);

// ── Distinctness ──
const a = nanoid();
const b = nanoid();
console.log("two ids distinct:", a !== b);

// ── customAlphabet ──
const digits = customAlphabet("0123456789", 6);
const d = digits();
console.log("custom digit length:", d.length);
console.log("custom digit only digits:", /^[0-9]+$/.test(d));

const hex = customAlphabet("abcdef0123456789", 12);
const h = hex();
console.log("custom hex length:", h.length);
console.log("custom hex only hex:", /^[a-f0-9]+$/.test(h));

/*
@covers
crates/perry-stdlib/src/nanoid.rs:
  - js_nanoid
  - js_nanoid_custom
  - js_nanoid_sized
*/
