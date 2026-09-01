// #9414: `Number.prototype.toLocaleString` and `Array.prototype.toLocaleString`
// ignored BOTH the locale argument and the options bag — `(1234.5)
// .toLocaleString("de-DE")` printed the en-US default "1,234.5" instead of
// node's "1.234,5", and `{style:"percent"}` / `{notation:"compact"}` were
// dropped entirely. ECMA-402 defines these as "construct an Intl.NumberFormat
// with exactly these arguments and format with it", so the fix is delegation;
// the `Intl.*` control rows below pin the delegation TARGET, so a divergence
// there is an Intl defect rather than a routing one.
//
// Every Date row pins `timeZone` so the expected bytes do not depend on the
// host zone. Compared byte-for-byte against `node --experimental-strip-types`.

const n = 1234.5;

// ---- locale is honored -----------------------------------------------------
console.log(n.toLocaleString("de-DE"));
console.log(n.toLocaleString("fr-FR"));
console.log(n.toLocaleString("ja-JP"));
console.log(n.toLocaleString("en-US"));
console.log((1234567.891).toLocaleString("de-DE"));
// `en-IN` is omitted on purpose: Perry's Intl.NumberFormat groups in fixed
// 3-digit runs, so `(1234567.891).toLocaleString("en-IN")` is "1,234,567.891"
// where node gives the Indian "12,34,567.891". That is a NumberFormat defect,
// not a delegation one — `new Intl.NumberFormat("en-IN").format(...)` is
// equally wrong standalone — so it is tracked separately rather than pinned
// here as a false failure.
// An unknown-but-well-formed tag falls back to the default locale.
console.log((1234567.891).toLocaleString("zz-ZZ"));

// ---- options bag is honored ------------------------------------------------
console.log((0.5).toLocaleString("en-US", { style: "percent" }));
console.log((0.1234).toLocaleString("de-DE", { style: "percent" }));
console.log((1234.5).toLocaleString("de-DE", { style: "currency", currency: "EUR" }));
console.log((1234.5).toLocaleString("en-US", { style: "currency", currency: "EUR" }));
console.log((1e6).toLocaleString("en-US", { notation: "compact" }));
console.log((1e6).toLocaleString("en-US", { notation: "compact", compactDisplay: "long" }));
console.log((1234.5).toLocaleString("en-US", { notation: "compact" }));
console.log((1234.5678).toLocaleString("en-US", { minimumFractionDigits: 2 }));
console.log((1234.5678).toLocaleString("en-US", { maximumFractionDigits: 2 }));
console.log((7).toLocaleString("en-US", { minimumIntegerDigits: 3 }));
console.log((1234.5).toLocaleString("en-US", { useGrouping: false }));

// An `undefined` locale with an options bag, and an empty locale list.
console.log((0.5).toLocaleString(undefined, { style: "percent" }));
console.log((1234.5678).toLocaleString(undefined, { maximumFractionDigits: 1 }));
console.log((1234.5).toLocaleString([], { minimumFractionDigits: 3 }));
console.log((1234.5).toLocaleString(["de-DE", "en-US"]));
console.log(Infinity.toLocaleString("en-US"));
console.log((-Infinity).toLocaleString("de-DE"));

// ---- control: the no-argument path must not change -------------------------
console.log((1234.5).toLocaleString());
console.log((12345).toLocaleString());
console.log((-9876543.21).toLocaleString());
console.log((0).toLocaleString());
console.log(NaN.toLocaleString());
console.log((2 ** 60).toLocaleString());

// ---- the delegation target, standalone -------------------------------------
console.log(new Intl.NumberFormat("de-DE").format(1234.5));
console.log(new Intl.NumberFormat("en-US", { style: "percent" }).format(0.5));
console.log(new Intl.NumberFormat("en-US", { notation: "compact" }).format(1e6));
console.log(new Intl.NumberFormat("fr-FR").format(1234.5));
console.log(new Intl.NumberFormat("ja-JP").format(1234.5));
console.log(new Intl.NumberFormat("de-DE", { style: "percent" }).format(0.1234));
console.log(new Intl.NumberFormat("de-DE", { style: "currency", currency: "EUR" }).format(1234.5));
console.log(new Intl.NumberFormat("en-US", { style: "currency", currency: "EUR" }).format(1234.5));
console.log(new Intl.NumberFormat("en-US", { notation: "compact", compactDisplay: "long" }).format(1e6));
console.log(new Intl.NumberFormat("en-US", { minimumIntegerDigits: 3 }).format(7));
console.log(new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 }).format(1234.5678));
// A locale whose DEFAULT (all-numeric) date pattern differs from en-US is
// omitted for the same reason: `new Intl.DateTimeFormat("de-DE").format(d)`
// gives "1/1/1970" instead of node's "1.1.1970" on its own, because
// `icu_dtf::format_components` deliberately declines a purely numeric field
// set and the caller's fallback assembly is hard-coded en-US. Everything
// below that names a spelled month, a weekday, a dateStyle or a timeStyle
// does reach the CLDR patterns and IS pinned.
console.log(new Intl.DateTimeFormat("en-US", { timeZone: "UTC" }).format(new Date(0)));

// ---- Date.prototype.toLocale{,Date,Time}String ------------------------------
const d = new Date(Date.UTC(2026, 8, 1, 14, 37, 9));
console.log(d.toLocaleDateString("en-US", { timeZone: "UTC" }));
console.log(d.toLocaleDateString("de-DE", { dateStyle: "long", timeZone: "UTC" }));
console.log(
  d.toLocaleDateString("de-DE", {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  }),
);
console.log(d.toLocaleTimeString("en-US", { timeStyle: "short", timeZone: "UTC" }));
console.log(d.toLocaleString("en-US", { timeZone: "UTC" }));
console.log(d.toLocaleString("ja-JP", { dateStyle: "full", timeStyle: "short", timeZone: "UTC" }));
console.log(d.toLocaleString("en-US", { timeZone: "Asia/Tokyo" }));
console.log(d.toLocaleDateString(undefined, { timeZone: "UTC", dateStyle: "short" }));

// ---- Array.prototype.toLocaleString forwards its arguments ------------------
console.log([1234.5, 6789.1].toLocaleString("de-DE"));
console.log([1234.5, 6789.1].toLocaleString("en-US"));
console.log([0.5, 0.25].toLocaleString("en-US", { style: "percent" }));
console.log([new Date(Date.UTC(2026, 8, 1))].toLocaleString("en-US", { timeZone: "UTC" }));
console.log([1234.5, null, undefined, 6789.1].toLocaleString("de-DE"));
// Control: the no-argument array form.
console.log([1234.5, 6789.1].toLocaleString());
console.log([3, 1, 2].toLocaleString());
