// #6960: two Intl parity gaps —
//   1. RelativeTimeFormat with numeric:"auto" must use CLDR relative word
//      forms (yesterday/today/tomorrow, last/this/next <unit>, now, …)
//      instead of always rendering the numeric form.
//   2. `instanceof Intl.<Ctor>` must be true for `class X extends Intl.<Ctor>`
//      subclass instances (OrdinaryHasInstance walks the real prototype
//      chain: X.prototype → Intl.<Ctor>.prototype).

const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

// day: the canonical three word forms
console.log("day -1 :", rtf.format(-1, "day"));
console.log("day  0 :", rtf.format(0, "day"));
console.log("day  1 :", rtf.format(1, "day"));
// day ±2 stays numeric
console.log("day -2 :", rtf.format(-2, "day"));
console.log("day  2 :", rtf.format(2, "day"));

// second / minute / hour: only the zero form is special
console.log("second 0:", rtf.format(0, "second"));
console.log("minute 0:", rtf.format(0, "minute"));
console.log("hour 0  :", rtf.format(0, "hour"));
console.log("second -1:", rtf.format(-1, "second"));

// week / month / quarter / year: last/this/next
console.log("week -1 :", rtf.format(-1, "week"));
console.log("week  0 :", rtf.format(0, "week"));
console.log("week  1 :", rtf.format(1, "week"));
console.log("month -1:", rtf.format(-1, "month"));
console.log("month  0:", rtf.format(0, "month"));
console.log("year -1 :", rtf.format(-1, "year"));
console.log("year  0 :", rtf.format(0, "year"));
console.log("year  1 :", rtf.format(1, "year"));
console.log("quarter 0:", rtf.format(0, "quarter"));

// numeric:"always" still produces the numeric form even for -1 day
const always = new Intl.RelativeTimeFormat("en", { numeric: "always" });
console.log("always day -1:", always.format(-1, "day"));

// short style auto forms abbreviate week/year
const short = new Intl.RelativeTimeFormat("en", { numeric: "auto", style: "short" });
console.log("short week -1:", short.format(-1, "week"));
console.log("short year  1:", short.format(1, "year"));
console.log("short day  -1:", short.format(-1, "day"));

// formatToParts for a word form is a single literal (no unit field)
const parts = rtf.formatToParts(-1, "day");
console.log(
  "parts day -1:",
  parts.map((p) => p.type + ":" + p.value + (p.unit ? "@" + p.unit : "")).join("|"),
);
// numeric form still attaches unit to the number part
const partsNum = rtf.formatToParts(-2, "day");
console.log(
  "parts day -2:",
  partsNum.map((p) => p.type + ":" + p.value + (p.unit ? "@" + p.unit : "")).join("|"),
);

// --- instanceof through an Intl subclass ---
class MyNF extends Intl.NumberFormat {}
const m = new MyNF("en-US");
console.log("subclass format:", m.format(99));
console.log("instanceof NumberFormat:", m instanceof Intl.NumberFormat);
console.log("instanceof MyNF:", m instanceof MyNF);
console.log("direct instanceof NumberFormat:", new Intl.NumberFormat("en-US") instanceof Intl.NumberFormat);
console.log("plain object instanceof:", ({} as any) instanceof Intl.NumberFormat);

class MyRTF extends Intl.RelativeTimeFormat {}
const mr = new MyRTF("en", { numeric: "auto" });
console.log("subclass RTF format:", mr.format(-1, "day"));
console.log("subclass RTF instanceof:", mr instanceof Intl.RelativeTimeFormat);
