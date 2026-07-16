// Intl.DateTimeFormat resolves the HOST time zone (like Node), applies its
// UTC offset when formatting a Date, honors an explicit UTC / numeric-offset
// zone, and Date.prototype.toLocaleString renders the instant in the resolved
// zone. Uses en-US + numeric hour/minute so the comparison isolates the time
// ZONE (offset) rather than any locale date-pattern differences, and avoids
// non-host named zones (whose offset needs the OS tz db). Byte-compared to
// `node --experimental-strip-types` on the same host, so both read the same
// host zone regardless of CI's timezone.
const d = new Date(Date.UTC(2026, 0, 15, 12, 30, 0));
const num = { hour: "2-digit", minute: "2-digit", hour12: false } as const;
console.log("resolvedIsString=" + (typeof new Intl.DateTimeFormat().resolvedOptions().timeZone === "string"));
console.log("resolvedMatchesGlobal=" +
  (new Intl.DateTimeFormat("en-US").resolvedOptions().timeZone ===
   new Intl.DateTimeFormat().resolvedOptions().timeZone));
console.log("utc=" + new Intl.DateTimeFormat("en-US", { timeZone: "UTC", ...num }).format(d));
console.log("host=" + new Intl.DateTimeFormat("en-US", num).format(d));
console.log("plus5=" + new Intl.DateTimeFormat("en-US", { timeZone: "+05:00", ...num }).format(d));
console.log("minus0330=" + new Intl.DateTimeFormat("en-US", { timeZone: "-03:30", ...num }).format(d));
console.log("toLocaleUtc=" + d.toLocaleString("en-US", { timeZone: "UTC", ...num }));
