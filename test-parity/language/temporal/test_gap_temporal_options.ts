// Temporal options-/fields-object methods (#4727): round/total, with*, and
// calendar conversions. Deterministic — byte-for-byte vs Node's Temporal
// (node --harmony-temporal / Node >=24).

// Duration.round / total
const d = Temporal.Duration.from({ hours: 2, minutes: 90 });
console.log(d.round({ largestUnit: "hours" }).toString());
console.log(d.round({ smallestUnit: "hours", roundingMode: "floor" }).toString());
console.log(d.total({ unit: "minutes" }));
console.log(Temporal.Duration.from({ minutes: 130 }).total("hours"));
console.log(
  Temporal.Duration.from({ months: 1, days: 15 }).total({
    unit: "days",
    relativeTo: "2020-01-01",
  }),
);

// PlainTime.with / round
const tm = Temporal.PlainTime.from("12:34:56");
console.log(tm.with({ minute: 0 }).toString());
console.log(tm.round({ smallestUnit: "hour" }).toString());
console.log(tm.round("minute").toString());
console.log(tm.round({ smallestUnit: "minute", roundingIncrement: 15 }).toString());

// PlainDate.with / withCalendar / toX
const pd = Temporal.PlainDate.from("2020-02-29");
console.log(pd.with({ year: 2021 }).toString());
console.log(pd.toPlainDateTime(Temporal.PlainTime.from("08:30")).toString());
console.log(pd.toPlainYearMonth().toString());
console.log(pd.toPlainMonthDay().toString());
console.log(pd.withCalendar("iso8601").calendarId);

// PlainDateTime.with / withPlainTime / round
const pdt = Temporal.PlainDateTime.from("2020-03-15T14:30:45");
console.log(pdt.with({ hour: 9 }).toString());
console.log(pdt.withPlainTime(Temporal.PlainTime.from("06:00")).toString());
console.log(pdt.round({ smallestUnit: "hour" }).toString());

// PlainYearMonth.with / toPlainDate
const ym = Temporal.PlainYearMonth.from("2020-06");
console.log(ym.with({ month: 12 }).toString());
console.log(ym.toPlainDate({ day: 15 }).toString());

// PlainMonthDay.with / toPlainDate
const md = Temporal.PlainMonthDay.from("--12-25");
console.log(md.with({ day: 31 }).toString());
console.log(md.toPlainDate({ year: 2024 }).toString());

// Instant.round / toZonedDateTimeISO
const inst = Temporal.Instant.from("2020-01-01T00:00:30Z");
console.log(inst.round({ smallestUnit: "minute" }).toString());
console.log(inst.toZonedDateTimeISO("UTC").toString());

// ZonedDateTime.with* / round / startOfDay / withTimeZone / getTimeZoneTransition
const zdt = Temporal.ZonedDateTime.from(
  "2020-06-15T12:00:00-04:00[America/New_York]",
);
console.log(zdt.with({ hour: 0 }).toString());
console.log(zdt.withCalendar("iso8601").calendarId);
console.log(zdt.round({ smallestUnit: "hour" }).toString());
console.log(zdt.startOfDay().toString());
console.log(zdt.withTimeZone("UTC").timeZoneId);
const jan = Temporal.ZonedDateTime.from(
  "2020-01-01T00:00:00-05:00[America/New_York]",
);
console.log(jan.getTimeZoneTransition("next").toString());
console.log(jan.getTimeZoneTransition("previous").toString());
console.log(
  Temporal.ZonedDateTime.from("2020-01-01T00:00:00+00:00[UTC]").getTimeZoneTransition(
    "next",
  ),
);
