// Temporal constructor overflow=reject + no integer-wrapping on time fields
// (#4686 review). Every out-of-range constructor arg must throw RangeError,
// matching Temporal-enabled Node (`node --harmony-temporal` / Node >= 24).
function expectThrow(label: string, fn: () => unknown): void {
  try {
    const v = fn();
    console.log("FAIL (no throw):", label, "->", String(v));
  } catch (e) {
    console.log("OK throws:", label);
  }
}

// PlainDate: month 13 must reject (was constrained to 2021-12-01).
expectThrow("PlainDate(2021,13,1)", () => new Temporal.PlainDate(2021, 13, 1));
expectThrow("PlainDate(2021,2,31)", () => new Temporal.PlainDate(2021, 2, 31));
// valid still works
console.log("valid PlainDate:", new Temporal.PlainDate(2021, 2, 28).toString());

// PlainDateTime: invalid day + wrapped hour must reject.
expectThrow("PlainDateTime(2021,2,31)", () => new Temporal.PlainDateTime(2021, 2, 31));
expectThrow("PlainDateTime hour=256 (wrap)", () => new Temporal.PlainDateTime(2021, 1, 1, 256));
expectThrow("PlainDateTime hour=-1", () => new Temporal.PlainDateTime(2021, 1, 1, -1));
console.log("valid PDT:", new Temporal.PlainDateTime(2021, 1, 1, 23, 59).toString());

// PlainYearMonth: month 13 reject.
expectThrow("PlainYearMonth(2021,13)", () => new Temporal.PlainYearMonth(2021, 13));
console.log("valid PYM:", new Temporal.PlainYearMonth(2021, 12).toString());

// PlainMonthDay: Feb 30 reject (was constrained to Feb 29).
expectThrow("PlainMonthDay(2,30)", () => new Temporal.PlainMonthDay(2, 30));
console.log("valid PMD:", new Temporal.PlainMonthDay(2, 29).toString());

// PlainTime: wrapping cases that previously slipped through.
expectThrow("PlainTime(256) wrap->0", () => new Temporal.PlainTime(256));
expectThrow("PlainTime(25)", () => new Temporal.PlainTime(25));
expectThrow("PlainTime(-1)", () => new Temporal.PlainTime(-1));
expectThrow("PlainTime(0,0,0,1000) ms", () => new Temporal.PlainTime(0, 0, 0, 1000));
console.log("valid PT:", new Temporal.PlainTime(23, 59, 59, 999).toString());
