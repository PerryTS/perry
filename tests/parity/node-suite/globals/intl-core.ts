function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", String(fn()));
  } catch (err: any) {
    console.log(label + ": throw", err?.constructor?.name ?? err?.name);
  }
}

const I: any = (globalThis as any).Intl;

show("namespace typeof", () => typeof I);
show("constructors typeof", () =>
  [typeof I.NumberFormat, typeof I.DateTimeFormat, typeof I.Collator].join(","),
);
show("ctor lengths", () => [I.NumberFormat.length, I.DateTimeFormat.length, I.Collator.length].join(","));

const deCurrency = new I.NumberFormat("de-DE", { style: "currency", currency: "EUR" });
show("number new format", () => deCurrency.format(1234.5));
show("number call format", () => I.NumberFormat("en-US", { maximumFractionDigits: 1 }).format(1234.56));
show("number resolved", () => {
  const opts = deCurrency.resolvedOptions();
  return [opts.locale, opts.style, opts.currency].join("|");
});

const utcShort = new I.DateTimeFormat("en-US", { dateStyle: "short", timeZone: "UTC" });
show("date new format", () => utcShort.format(new Date("2020-01-02T00:00:00Z")));
show("date call format", () =>
  I.DateTimeFormat("en-US", { dateStyle: "short", timeZone: "UTC" }).format(new Date(Date.UTC(2020, 0, 2))),
);
show("date resolved", () => {
  const opts = utcShort.resolvedOptions();
  return [opts.locale, opts.dateStyle, opts.timeZone].join("|");
});

const svCollator = new I.Collator("sv");
show("collator new compare", () => svCollator.compare("\u00e4", "z"));
show("collator call compare", () => I.Collator("en").compare("a", "b"));
show("collator resolved", () => svCollator.resolvedOptions().locale);

show("supported locales", () =>
  [
    I.NumberFormat.supportedLocalesOf(["de-DE", "en-US"]).join("|"),
    I.DateTimeFormat.supportedLocalesOf(["en-US", "sv"]).join("|"),
    I.Collator.supportedLocalesOf(["sv", "en"]).join("|"),
  ].join(";"),
);
