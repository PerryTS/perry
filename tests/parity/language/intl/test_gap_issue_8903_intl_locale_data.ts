// Issue #8903: DateTimeFormat and Collator must use the locale and time-zone
// data they report from resolvedOptions(). The fixed instant is in CEST so an
// implementation that silently formats it as UTC is two hours behind.
const instant = new Date("2026-09-07T06:05:00Z");

const mediumDate = new Intl.DateTimeFormat("de-DE", {
  dateStyle: "medium",
  timeZone: "Europe/Berlin",
});
const weekday = new Intl.DateTimeFormat("de-DE", {
  weekday: "long",
  timeZone: "Europe/Berlin",
});
const berlinTime = new Intl.DateTimeFormat("de-DE", {
  timeStyle: "short",
  timeZone: "Europe/Berlin",
});

console.log(mediumDate.resolvedOptions().locale, mediumDate.format(instant));
console.log(weekday.resolvedOptions().locale, weekday.format(instant));
console.log(berlinTime.resolvedOptions().timeZone, berlinTime.format(instant));
console.log(
  ["Zubehör", "Ärger", "Apfel"].sort(new Intl.Collator("de-DE").compare).join(", "),
);
