// Temporal.PlainDate (#4691) + Temporal.PlainTime (#4692) parity.
const d = new Temporal.PlainDate(2026, 6, 6);
console.log(d.toString());
console.log(d.year, d.month, d.day, d.monthCode, d.dayOfWeek, d.dayOfYear);
console.log(d.daysInMonth, d.daysInYear, d.monthsInYear, d.inLeapYear, d.calendarId);
console.log(d.add(new Temporal.Duration(0, 1)).toString());
console.log(d.subtract(new Temporal.Duration(0, 0, 0, 5)).toString());
console.log(d.until(new Temporal.PlainDate(2026, 12, 25)).toString());
console.log(Temporal.PlainDate.compare(d, new Temporal.PlainDate(2026, 7, 6)));
console.log(d.equals(new Temporal.PlainDate(2026, 6, 6)));
console.log(Temporal.PlainDate.from("2020-02-29").toString());
console.log(JSON.stringify({ d }));

const t = new Temporal.PlainTime(13, 45, 30, 500);
console.log(t.toString());
console.log(t.hour, t.minute, t.second, t.millisecond, t.microsecond, t.nanosecond);
console.log(t.add(new Temporal.Duration(0, 0, 0, 0, 0, 90)).toString());
console.log(t.subtract(new Temporal.Duration(0, 0, 0, 0, 1)).toString());
console.log(Temporal.PlainTime.compare(t, new Temporal.PlainTime(9)));
console.log(t.equals(new Temporal.PlainTime(13, 45, 30, 500)));
console.log(Temporal.PlainTime.from("23:59:59").toString());
