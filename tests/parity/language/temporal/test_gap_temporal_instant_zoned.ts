// Temporal.Instant (#4690) + Temporal.ZonedDateTime (#4695) parity.
const i = new Temporal.Instant(1717689600000000000n);
console.log(i.toString());
console.log(i.epochMilliseconds, i.epochNanoseconds, typeof i.epochNanoseconds);
console.log(Temporal.Instant.fromEpochMilliseconds(0).toString());
console.log(Temporal.Instant.from("2020-01-01T00:00:00Z").toString());
console.log(
  Temporal.Instant.compare(
    Temporal.Instant.fromEpochMilliseconds(0),
    Temporal.Instant.from("2020-01-01T00:00:00Z"),
  ),
);
console.log(Temporal.Instant.fromEpochMilliseconds(0).add(new Temporal.Duration(0, 0, 0, 0, 1)).toString());
console.log(Temporal.Instant.fromEpochMilliseconds(0).equals(Temporal.Instant.fromEpochMilliseconds(0)));

const z = new Temporal.ZonedDateTime(0n, "America/New_York");
console.log(z.toString());
console.log(z.year, z.month, z.day, z.hour, z.timeZoneId, z.offset, z.offsetNanoseconds);
console.log(z.epochMilliseconds, z.epochNanoseconds, z.calendarId, z.hoursInDay);

const utc = new Temporal.ZonedDateTime(0n, "UTC");
console.log(utc.toString());
console.log(utc.toInstant().toString(), utc.toPlainDate().toString(), utc.toPlainTime().toString());
console.log(utc.toPlainDateTime().toString());
console.log(utc.add(new Temporal.Duration(0, 0, 0, 1)).toString());
console.log(Temporal.ZonedDateTime.compare(z, utc), utc.equals(new Temporal.ZonedDateTime(0n, "UTC")));
// Spring-forward day in New York is 23 hours long.
console.log(new Temporal.ZonedDateTime(1710046800000000000n, "America/New_York").hoursInDay);
