// Temporal.Duration parity (#4688). Deterministic — byte-for-byte vs Node's
// Temporal (node --harmony-temporal / Node >=24).
const d = new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9);
console.log(d.toString());
console.log(d.years, d.months, d.weeks, d.days, d.hours, d.minutes, d.seconds);
console.log(d.milliseconds, d.microseconds, d.nanoseconds, d.sign, d.blank);
console.log(new Temporal.Duration().blank, new Temporal.Duration().sign);
console.log(Temporal.Duration.from("P1Y2M3DT4H5M6S").toString());
console.log(Temporal.Duration.from({ hours: 10, minutes: 30 }).toString());

const t = new Temporal.Duration(0, 0, 0, 0, 5, 30);
console.log(t.add(new Temporal.Duration(0, 0, 0, 0, 1)).hours);
console.log(t.subtract(new Temporal.Duration(0, 0, 0, 0, 2)).hours);
console.log(t.negated().sign, t.negated().abs().sign);
console.log(t.with({ hours: 100 }).hours, t.with({ hours: 100 }).minutes);
console.log(
  Temporal.Duration.compare(
    new Temporal.Duration(0, 0, 0, 0, 1),
    new Temporal.Duration(0, 0, 0, 0, 2),
  ),
);
console.log(JSON.stringify({ d: Temporal.Duration.from("P1Y2M3DT4H5M6S") }));
