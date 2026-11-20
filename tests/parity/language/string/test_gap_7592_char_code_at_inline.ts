// #7592: `String.prototype.charCodeAt` on a statically-string receiver now
// lowers to an inline guarded byte load instead of two opaque runtime calls
// (`js_string_index_to_i32` + `js_string_char_code_at`). Everything the guard
// chain cannot prove — a short (SSO) receiver, a non-ASCII payload, a lone
// surrogate, an out-of-range or non-numeric index — must still take the
// runtime arm and produce exactly the spec result.
//
// The receiver is passed as a `string` parameter on purpose: that is the shape
// that takes the inline path (`is_string_expr` on a parameter), and it is the
// shape `honest_bench`'s FNV-1a hash loop uses.

function at(s: string, i: number): number {
  return s.charCodeAt(i);
}

function atAny(s: string, i: any): number {
  return s.charCodeAt(i);
}

// ── ASCII heap string: the fast arm ───────────────────────────────────────
let ascii = '';
for (let i = 0; i < 40; i++) ascii += String.fromCharCode(65 + (i % 26));
console.log('ascii len', ascii.length);
console.log('ascii 0', at(ascii, 0));
console.log('ascii 25', at(ascii, 25));
console.log('ascii last', at(ascii, ascii.length - 1));

// Out of range on both ends -> NaN (never a byte read past the payload).
console.log('ascii len-index', at(ascii, ascii.length));
console.log('ascii huge', at(ascii, 1e9));
console.log('ascii -1', at(ascii, -1));
console.log('ascii -1e12', at(ascii, -1e12));

// ToIntegerOrInfinity on the index: truncation toward zero, NaN -> 0.
console.log('ascii 3.9', at(ascii, 3.9));
console.log('ascii -0.5', at(ascii, -0.5));
console.log('ascii NaN', at(ascii, NaN));
console.log('ascii Infinity', at(ascii, Infinity));
console.log('ascii -Infinity', at(ascii, -Infinity));

// Non-numeric indices must still run the full coercion.
console.log('ascii "2"', atAny(ascii, '2'));
console.log('ascii true', atAny(ascii, true));
console.log('ascii null', atAny(ascii, null));
console.log('ascii undefined', atAny(ascii, undefined));
console.log('ascii {valueOf}', atAny(ascii, { valueOf: () => 4 }));
console.log('ascii no-arg', (ascii as any).charCodeAt());

// ── Short (SSO) receiver: guard must route to the runtime arm ────────────
const sso = 'ab';
console.log('sso 0', at(sso, 0));
console.log('sso 1', at(sso, 1));
console.log('sso 2', at(sso, 2));
console.log('empty 0', at('', 0));

// ── Non-ASCII payloads: utf16_len != byte_len, runtime arm ───────────────
const accented = 'héllo wörld';
console.log('accented len', accented.length);
for (let i = 0; i < accented.length; i++) {
  console.log('accented', i, at(accented, i));
}

// Astral code point -> two UTF-16 code units.
const astral = 'a\u{1F600}b';
console.log('astral len', astral.length);
console.log('astral 0', at(astral, 0));
console.log('astral 1', at(astral, 1));
console.log('astral 2', at(astral, 2));
console.log('astral 3', at(astral, 3));
console.log('astral 4', at(astral, 4));

// Lone surrogate (WTF-8 payload).
const lone = 'x\uD800y';
console.log('lone len', lone.length);
console.log('lone 0', at(lone, 0));
console.log('lone 1', at(lone, 1));
console.log('lone 2', at(lone, 2));

// A high-byte binary string: every code unit is one byte 0..255, but bytes
// >= 128 make byte_len > utf16_len, so this must NOT take the ASCII arm.
let bytes = '';
for (let i = 0; i < 256; i++) bytes += String.fromCharCode(i);
let roundTrip = true;
for (let i = 0; i < 256 && roundTrip; i++) roundTrip = at(bytes, i) === i;
console.log('binary 0-255 round-trip', roundTrip);

// ── The hash-loop shape itself ───────────────────────────────────────────
function imul32(a: number, b: number): number {
  const aHi = (a >>> 16) & 0xffff;
  const aLo = a & 0xffff;
  const bHi = (b >>> 16) & 0xffff;
  const bLo = b & 0xffff;
  return (aLo * bLo + (((aHi * bLo + aLo * bHi) << 16) >>> 0)) | 0;
}
function fnv1a32(s: string): number {
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < s.length; i++) {
    h = (h ^ s.charCodeAt(i)) | 0;
    h = imul32(h, 0x01000193);
  }
  return h >>> 0;
}
console.log('fnv ascii', fnv1a32(ascii).toString(16));
console.log('fnv accented', fnv1a32(accented).toString(16));
console.log('fnv astral', fnv1a32(astral).toString(16));
console.log('fnv binary', fnv1a32(bytes).toString(16));
console.log('fnv empty', fnv1a32('').toString(16));

// A nullable-string receiver still reaches the string lowering.
function atNullable(s: string | null, i: number): number {
  return s === null ? -1 : s.charCodeAt(i);
}
console.log('nullable', atNullable(ascii, 1), atNullable(null, 1));

// The other Number-returning String methods that #7592 also marks numeric —
// each xored so the inline bitwise path is exercised, not just the call.
console.log('indexOf xor', (0 ^ ascii.indexOf('C')) | 0);
console.log('lastIndexOf xor', (0 ^ ascii.lastIndexOf('C')) | 0);
console.log('search xor', (0 ^ ascii.search(/C/)) | 0);
console.log('localeCompare', 'a'.localeCompare('b'), 'b'.localeCompare('a'), 'a'.localeCompare('a'));
console.log('indexOf missing', (0 ^ ascii.indexOf('zzz')) | 0);

// `codePointAt` is deliberately NOT claimed numeric: it returns `undefined`
// out of range, which is a NaN-boxed tag rather than a number.
console.log('codePointAt 0', ascii.codePointAt(0));
console.log('codePointAt oob', ascii.codePointAt(1000));
