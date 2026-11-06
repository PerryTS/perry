// #5899 — CreateDateTimeFormat ResolveLocale for the `ca`, `hc`, and `nu`
// relevant extension keys. Unsupported values fall back, supported extensions
// survive only when they are actually selected, explicit options override the
// extension, and unrelated Unicode keys never leak into the resolved locale.

function show(
  label: string,
  locale: string,
  options: Intl.DateTimeFormatOptions = {},
): void {
  const resolved = new Intl.DateTimeFormat(locale, options).resolvedOptions();
  console.log(
    label,
    resolved.locale,
    resolved.calendar,
    resolved.numberingSystem,
    resolved.hourCycle ?? "-",
  );
}

show("future option", "en", { calendar: "bangla" });
show("future extension", "en-u-ca-vikram");
show("calendar alias", "en", { calendar: "ethiopic-amete-alem" });
show("keep calendar", "en-u-ca-iso8601", { calendar: "invalid" });
show("drop calendars", "en-u-ca-invalid", { calendar: "invalid2" });
show("replace calendar", "en-u-ca-gregory", { calendar: "iso8601" });
show("same calendar", "en-u-ca-iso8601", { calendar: "iso8601" });
show("null calendar", "en-u-ca-iso8601", {
  calendar: null as unknown as string,
});

show("irrelevant", "ja-JP-u-cu-usd-tz-usnyc");
show("numbering", "en-u-nu-arab");
show("replace hc", "en-u-hc-h23", { hour: "numeric", hourCycle: "h11" });
show("same hc", "en-u-hc-h23", { hour: "numeric", hourCycle: "h23" });
show("hour12 wins", "en-u-hc-h11", {
  hour: "numeric",
  hour12: false,
  hourCycle: "h11",
});
show("extension hc", "en-u-hc-h11", { hour: "numeric" });
