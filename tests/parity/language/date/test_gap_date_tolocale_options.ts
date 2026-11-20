// `Date.prototype.toLocaleDateString` / `toLocaleTimeString` must honor their
// locale + options arguments. Perry used to fold every call (even arg'd ones)
// to a fixed default formatter, so e.g.
//   new Date().toLocaleDateString("en-US", { month: "short", day: "numeric" })
// printed the numeric default ("1/5/2026") instead of Node's "Jan 5". Only the
// zero-arg fast path may skip the Intl formatter now; arg'd calls route through
// the real prototype thunks (date-only / time-only default field classes).
//
// timeZone:"UTC" keeps the output independent of the host time zone. Compared
// byte-for-byte against `node --experimental-strip-types`.
const d = new Date(Date.UTC(2026, 0, 5, 14, 37, 9));

console.log(d.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" }));
console.log(
  d.toLocaleDateString("en-US", {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  }),
);
console.log(d.toLocaleDateString("en-US", { dateStyle: "medium", timeZone: "UTC" }));
console.log(d.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" }));
console.log(
  d.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "numeric",
    second: "numeric",
    timeZone: "UTC",
  }),
);
console.log(d.toLocaleTimeString("en-US", { timeStyle: "short", timeZone: "UTC" }));
