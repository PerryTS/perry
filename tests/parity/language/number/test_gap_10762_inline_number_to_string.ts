// #10762: `String(n)`, `${n}`, `n.toString()` and `"" + n` build a number
// operand's small-integer text inline at the call site, and `s.charCodeAt(i)`
// reads an ASCII short string's byte straight out of the value. Both have a
// runtime fallback; this pins the boundary between the two arms and the
// fallbacks themselves.

function spell(n: number): string {
  const a = String(n);
  const b = `${n}`;
  const c = n.toString();
  const d = "" + n;
  const agree = a === b && b === c && c === d;
  return agree ? a + "|" + a.length : "DISAGREE " + [a, b, c, d].join(",");
}

// Around every edge of the inline range (-9999..=99999) and each digit count.
const edges: number[] = [
  0, -0, 1, -1, 9, 10, -9, -10, 99, 100, -99, -100, 999, 1000, -999, -1000,
  9999, 10000, -9999, -10000, 99999, 100000, -99999, 99998.5, -9998.5,
  0.5, -0.5, 1e-7, 1e21, NaN, Infinity, -Infinity, 2147483648, -2147483649,
];
for (const n of edges) console.log(n, spell(n));

// Every integer the inline arm covers, checked against a digit-by-digit
// reference; only a count and the first mismatch are printed.
function reference(n: number): string {
  if (n === 0) return "0";
  let rest = Math.abs(n);
  let out = "";
  while (rest > 0) {
    out = String.fromCharCode(48 + (rest % 10)) + out;
    rest = Math.floor(rest / 10);
  }
  return n < 0 ? "-" + out : out;
}
let checked = 0;
let firstBad = "";
for (let n = -9999; n <= 99999; n++) {
  const got = String(n);
  if (got !== reference(n) || `${n}` !== got || n.toString() !== got || "" + n !== got) {
    if (firstBad === "") firstBad = n + " -> " + got;
  }
  checked++;
}
console.log("checked", checked, firstBad === "" ? "all match" : "first mismatch " + firstBad);

// A `number` annotation is not enforced at run time: the inline arm must
// decline anything that is not a plain double and take the full conversion.
const liars: any[] = ["abc", "12", true, null, undefined, 12n, [1, 2], { a: 1 }];
for (const v of liars) {
  const n: number = v;
  console.log(String(n), `${n}`, "" + n);
}
// (`"" + valueOfOnly` is left out: it prints "seven" instead of "7" on main
// as well, a separate ToPrimitive-hint bug in the declared-number concat.)
const valueOfOnly: number = { valueOf: () => 7, toString: () => "seven" } as any;
console.log(String(valueOfOnly), `${valueOfOnly}`);

// Integer-valued results from arithmetic, loop counters and Int32 locals.
let acc = 0;
for (let i = -3; i < 4; i++) acc += String(i * 7).length + `${i | 0}`.length;
console.log(acc, String(2 ** 16), `${(3 * 33333) | 0}`, (65535 & 0xffff).toString());

// charCodeAt on short strings: numbers, ASCII words, non-ASCII, bad indexes.
const shorts = [String(42), `${-7}`, "" + 12345, "abcde", "é", "aé", "€", ""];
for (const s of shorts) {
  const codes: (number | string)[] = [];
  for (let i = -1; i <= s.length; i++) {
    const c = s.charCodeAt(i);
    codes.push(Number.isNaN(c) ? "NaN" : c);
  }
  console.log(JSON.stringify(s), codes.join(","), s.charCodeAt(0.9), s.charCodeAt(1.5));
}
const idxLiar: number = "1" as any;
console.log(String(123).charCodeAt(idxLiar), String(123).charCodeAt(NaN));
