// #9450: Intl.NumberFormat grouped every locale in fixed 3-digit runs.
// CLDR's en-IN pattern has a 3-digit primary group and 2-digit secondary
// groups. Compared byte-for-byte against `node --experimental-strip-types`.

const indianValues = [1234, 12345, 123456, 1234567, 123456789];
const indian = new Intl.NumberFormat("en-IN");

for (const value of indianValues) {
    console.log("Intl en-IN " + value + ": " + indian.format(value));
}

// Pin typed group/integer parts, not only their concatenated spelling.
console.log(
    "parts en-IN: " +
        JSON.stringify(indian.formatToParts(123456789).map(({ type, value }) => [type, value])),
);

// Three-digit controls must remain unchanged, including distinct separators.
for (const locale of ["en-US", "de-DE", "fr-FR"]) {
    console.log(locale + ": " + new Intl.NumberFormat(locale).format(123456789));
}

console.log(
    "en-IN ungrouped: " +
        new Intl.NumberFormat("en-IN", { useGrouping: false }).format(123456789),
);

// Number.prototype.toLocaleString delegates to the same formatter and must
// preserve the locale's primary/secondary pattern at every divergent width.
for (const value of indianValues) {
    console.log("toLocaleString en-IN " + value + ": " + value.toLocaleString("en-IN"));
}

// BigInt has a separate exact-precision rendering path; it consumes the same
// grouping metadata and must not quietly retain fixed 3-digit runs.
console.log(
    "BigInt en-IN: " +
        (12345678901234567890n as any).toLocaleString("en-IN"),
);

