// Temporal.PlainDateTime (#4693) + PlainYearMonth/PlainMonthDay (#4694) parity.
const dt = new Temporal.PlainDateTime(2026, 6, 6, 13, 45, 30, 500);
console.log(dt.toString());
console.log(dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, dt.millisecond);
console.log(dt.dayOfWeek, dt.calendarId, dt.daysInMonth, dt.inLeapYear);
console.log(dt.toPlainDate().toString(), dt.toPlainTime().toString());
console.log(dt.add(new Temporal.Duration(0, 0, 0, 0, 2)).toString());
console.log(Temporal.PlainDateTime.from("2020-12-25T09:30:00").toString());
console.log(Temporal.PlainDateTime.compare(dt, new Temporal.PlainDateTime(2020, 1, 1)));
console.log(dt.equals(new Temporal.PlainDateTime(2026, 6, 6, 13, 45, 30, 500)));

const ym = new Temporal.PlainYearMonth(2026, 6);
console.log(ym.toString(), ym.year, ym.month, ym.daysInMonth, ym.monthCode);
console.log(ym.add(new Temporal.Duration(0, 3)).toString());
console.log(Temporal.PlainYearMonth.from("2024-02").inLeapYear);
console.log(ym.equals(new Temporal.PlainYearMonth(2026, 6)));

const md = new Temporal.PlainMonthDay(12, 25);
console.log(md.toString(), md.monthCode, md.day);
console.log(md.equals(new Temporal.PlainMonthDay(12, 25)));
console.log(Temporal.PlainMonthDay.from("--02-29").toString());
