// charCodeAt results used as array indices. A charCodeAt on a proven String
// is a UTF-16 code unit or NaN (index out of range), so the array key must
// stay correct for NaN (a property named "NaN"), for writes that leave int32,
// and for receivers that are not Strings.

const table: any[] = [];
for (let k = 0; k < 256; ++k) table[k] = "%" + (k < 16 ? "0" : "") + k.toString(16).toUpperCase();
const withNaN: any[] = table.slice();
(withNaN as any)["NaN"] = "<NaN prop>";
(withNaN as any)["1.5"] = "<1.5 prop>";
(withNaN as any)["-1"] = "<-1 prop>";
const big: any[] = [];
for (let k = 0; k < 0x10000; ++k) big[k] = k & 0xff;

// 1. In range and past the end (NaN index -> undefined / the "NaN" property).
function pastEnd(input: any, t: any[]) {
  const s = String(input);
  const out: any[] = [];
  for (var i = 0; i < s.length + 2; ++i) {
    var c = s.charCodeAt(i);
    out.push(t[c]);
  }
  return out;
}
console.log(JSON.stringify(pastEnd("aZ~", table)));
console.log(JSON.stringify(pastEnd("aZ~", withNaN)));
console.log(JSON.stringify(pastEnd(123, withNaN)));

// 2. Negative and fractional indices give NaN too.
function oddIndex(input: any, t: any[]) {
  const s = String(input);
  var a = s.charCodeAt(-1);
  var b = s.charCodeAt(0.9);
  var d = s.charCodeAt(1e10);
  return [t[a], t[b], t[d], a !== a, b, d !== d];
}
console.log(JSON.stringify(oddIndex("xy", table)));
console.log(JSON.stringify(oddIndex("xy", withNaN)));

// 3. qs shape: a hoisted var receiver built by a ternary and slice, a hoisted
//    `var c` written by charCodeAt and by a bounded surrogate combination.
var limit = 4;
function encodeLike(str: any) {
  if (str.length === 0) return str;
  var string = String(str);
  var out = "";
  for (var j = 0; j < string.length; j += limit) {
    var segment = string.length >= limit ? string.slice(j, j + limit) : string;
    if (j + limit < string.length) {
      var last = segment.charCodeAt(segment.length - 1);
      if (last >= 0xd800 && last <= 0xdbff) {
        segment = segment.slice(0, -1);
        j -= 1;
      }
    }
    var arr: any[] = [];
    for (var i = 0; i < segment.length; ++i) {
      var c = segment.charCodeAt(i);
      if (c < 0x80) {
        arr[arr.length] = table[c];
        continue;
      }
      if (c < 0x800) {
        arr[arr.length] = table[0xc0 | (c >> 6)] + table[0x80 | (c & 0x3f)];
        continue;
      }
      if (c < 0xd800 || c >= 0xe000) {
        arr[arr.length] = table[0xe0 | (c >> 12)] + table[0x80 | ((c >> 6) & 0x3f)] + table[0x80 | (c & 0x3f)];
        continue;
      }
      i += 1;
      c = 0x10000 + (((c & 0x3ff) << 10) | (segment.charCodeAt(i) & 0x3ff));
      arr[arr.length] = table[0xf0 | (c >> 18)] + table[0x80 | ((c >> 12) & 0x3f)]
        + table[0x80 | ((c >> 6) & 0x3f)] + table[0x80 | (c & 0x3f)] + "|" + big[c];
    }
    out += arr.join("");
  }
  return out;
}
for (const v of ["abc", "a b&c=d", "été", "€10", "x😀y", "😀😀", "lone\ud83d", "\udc00z", 42, true]) {
  console.log(JSON.stringify(encodeLike(v)));
}

// 4. A receiver that is not a String: a user charCodeAt is not a code unit.
const odd = { charCodeAt(_i: number) { return 1.5; } };
const neg = { charCodeAt(_i: number) { return -1; } };
const huge = { charCodeAt(_i: number) { return 4294967297; } };
function anyRecv(x: any, t: any[]) {
  var c = x.charCodeAt(0);
  return t[c];
}
console.log(JSON.stringify([anyRecv("A", withNaN), anyRecv(odd, withNaN), anyRecv(neg, withNaN), anyRecv(huge, withNaN), anyRecv(odd, table)]));
function mixedWrites(input: any, flip: boolean, t: any[]) {
  const s = String(input);
  var c = s.charCodeAt(0);
  if (flip) c = odd.charCodeAt(0);
  return t[c];
}
console.log(JSON.stringify([mixedWrites("B", false, withNaN), mixedWrites("B", true, withNaN)]));

// 5. A write that leaves int32: not a code unit.
function halves(input: any, t: any[]) {
  const s = String(input);
  const out: any[] = [];
  for (var i = 0; i < s.length; ++i) {
    var c = s.charCodeAt(i);
    if (c > 0x60) c = c / 2;
    out.push(t[c]);
  }
  return out;
}
console.log(JSON.stringify(halves("aB", withNaN)));
function shifted(input: any, t: any[]) {
  const s = String(input);
  var c = s.charCodeAt(0);
  c = c * 65536 * 65536;
  return [t[c], c];
}
console.log(JSON.stringify(shifted("\u0001", withNaN)));

// 6. Surrogate code units index a 64K table directly.
function units(input: any) {
  const s = String(input);
  const out: any[] = [];
  for (var i = 0; i <= s.length; ++i) {
    var c = s.charCodeAt(i);
    out.push(big[c], c);
  }
  return out;
}
console.log(JSON.stringify(units("😀￿")));

// 7. A redeclared parameter is aliased by a sloppy mapped arguments object.
function redeclared(k: any, input: any) {
  var s = String(input);
  var k = s.charCodeAt(0);
  (arguments as any)[0] = 1.5;
  return withNaN[k];
}
console.log(JSON.stringify([redeclared(0, "x"), redeclared(0, "")]));
