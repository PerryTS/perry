// #9414: `new Date("2026/09/01")` / `new Date("2026/9/1")` / `new Date("09/01/2026")`
// were all Invalid Date in Perry; node accepts the numeric slash form as its
// ECMA-262 §21.4.3.2 implementation-defined format. The spec deliberately says
// nothing here, so every expectation below is measured against
// `node --experimental-strip-types`, not derived from the standard.
//
// The subtle half is that these components are LOCAL wall-clock time, unlike
// the ISO branch, which is UTC — so this asserts the local getters (which are
// host-zone independent) and, for the zone-designated rows, `toISOString()`.
// `getTime()` itself is deliberately NOT printed: the local getters below
// fully determine the instant in the host zone, and a raw epoch (or a delta
// against any fixed reference) would couple the output to the host zone's
// DST regime at each row's date.

function show(input: string): void {
  const d = new Date(input);
  const t = d.getTime();
  if (Number.isNaN(t)) {
    console.log(input, "=> Invalid Date");
    return;
  }
  console.log(
    input,
    "=>",
    d.getFullYear(),
    d.getMonth(),
    d.getDate(),
    d.getHours(),
    d.getMinutes(),
    d.getSeconds(),
    d.getMilliseconds(),
  );
}

// The three shapes from the issue.
show("2026/09/01");
show("2026/9/1");
show("09/01/2026");
show("9/1/2026");

// Two-digit years: 0..49 → 2000s, 50..99 → 1900s.
show("09/01/26");
show("1/2/26");
show("99/1/1");
show("70/1/1");
show("49/1/1");
show("50/1/1");
show("00/1/1");

// Out-of-range components. A day past the end of its month ROLLS OVER; an
// out-of-range month, a zero month and a zero day are Invalid Date.
show("2026/13/01");
show("2026/00/01");
show("2026/09/00");
show("2026/09/32");
show("2026/02/30");
show("2026/09/31");
show("31/1/2026");
show("13/1/2026");
show("0/1/2026");
show("9/2026");
show("2026/1/2/3");

// With a clock. `T` is ISO-only and is NOT accepted by this grammar.
show("2026/09/01 10:30");
show("2026/09/01T10:30");
show("09/01/2026 10:30");
show("2026/09/01 10:30:45");
show("2026/09/01 10:30:45.123");
show("2026/09/01,10:30");
show("2026/09/01 24:00");
show("2026/09/01 25:00");
show("2026/09/01 10:60");
show("2026/09/01 3:04 PM");
show("2026/09/01 12:30 am");
show("2026/09/01 12:30 pm");
show("  2026/09/01  ");
show("2026/9");
show("Sep/01/2026");

// Zone-designated rows are absolute instants, so compare the ISO form.
for (
  const s of [
    "2026/09/01 GMT",
    "09/01/2026 GMT",
    "2026/09/01 10:30 UTC",
    "2026/09/01 10:30 GMT+0500",
    "2026/9/1 10:30:45.123 UTC",
    "2026/09/01 +0500",
    "1/1/100 GMT",
  ]
) {
  const d = new Date(s);
  console.log(s, "=>", Number.isNaN(d.getTime()) ? "Invalid Date" : d.toISOString());
}

// `Date.parse` is the same grammar.
console.log(Date.parse("2026/09/01") === new Date("2026/09/01").getTime());
console.log(Number.isNaN(Date.parse("2026/13/01")));

// Control: the ISO and RFC grammars must not change.
console.log(new Date("2026-09-01").toISOString());
console.log(new Date("2026-09-01T10:30:00Z").toISOString());
console.log(new Date("Thu, 01 Jan 1970 00:00:00 GMT").toISOString());
console.log(new Date("2020").toISOString());
console.log(String(new Date("not a date")));
