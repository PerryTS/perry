// Issue #748 — `new Date(NaN)` / `: Date` object-method must be a Date
// object, not a bare number. Mirrors the comment's minimal repro plus
// regression coverage for valid dates.

// --- the comment's deterministic minimal repro ---
function w5(y: number) {
  return {
    toDate(): Date {
      return new Date(NaN);
    },
  };
}
console.log("w5 typeof:", typeof w5(2026).toDate()); // expect: object

// --- core Invalid Date identity ---
const inv = new Date(NaN);
console.log("typeof inv:", typeof inv); // object
console.log("inv instanceof Date:", inv instanceof Date); // true
console.log("inv instanceof Object:", inv instanceof Object); // true
console.log("inv.getTime() isNaN:", Number.isNaN(inv.getTime())); // true
console.log("inv.getFullYear() isNaN:", Number.isNaN(inv.getFullYear())); // true
console.log("String(inv):", String(inv.toISOString())); // Invalid Date
console.log("inv.toDateString():", inv.toDateString()); // Invalid Date
console.log("JSON.stringify(inv):", JSON.stringify(inv)); // null
console.log("JSON.stringify({d:inv}):", JSON.stringify({ d: inv })); // {"d":null}

// --- invalid via string / Date.UTC(NaN) ---
const invStr = new Date("not a date");
console.log("invStr typeof:", typeof invStr); // object
console.log("invStr instanceof Date:", invStr instanceof Date); // true

// --- the @perryts/mysql MyDateTime.toDate() shape (no mysql) ---
function makeMyDateTime(y: number, mo: number, d: number) {
  return {
    toDate(): Date {
      if (y === 0 && mo === 0 && d === 0) return new Date(NaN);
      return new Date(Date.UTC(y, mo - 1, d, 7, 47, 4, 192));
    },
  };
}
function dtToIso(v: { toDate(): Date }): string | null {
  const dt = v.toDate();
  const t = dt.getTime();
  return Number.isNaN(t) ? null : new Date(t).toISOString();
}
console.log("valid dtToIso:", dtToIso(makeMyDateTime(2026, 5, 15)));
console.log("zero  dtToIso:", dtToIso(makeMyDateTime(0, 0, 0))); // null

// --- regression: valid Dates must be completely unaffected ---
const v = new Date(Date.UTC(2026, 4, 15, 17, 29, 35, 402));
console.log("v typeof:", typeof v); // object
console.log("v instanceof Date:", v instanceof Date); // true
console.log("v instanceof Object:", v instanceof Object); // true
console.log("v.getFullYear():", v.getUTCFullYear()); // 2026
console.log("v.getTime():", v.getTime()); // 1778880575402
console.log("v.toISOString():", v.toISOString()); // 2026-05-15T17:29:35.402Z
console.log("JSON.stringify(v):", JSON.stringify(v)); // "2026-05-15T17:29:35.402Z"
const n = 1778880575402; // plain number equal to v's millis must stay a number
console.log("plain number typeof:", typeof n); // number
console.log("Date.now() typeof:", typeof Date.now()); // number
console.log("(new Date()) instanceof Date:", new Date() instanceof Date); // true
