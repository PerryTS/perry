# Locale-Aware Formatting

Perry provides format wrapper functions that automatically format values according to the current locale. Import them from `perry/i18n`:

```typescript
{{#include ../../examples/i18n/snippets.ts:format-imports}}
```

## Format Wrappers

### Currency

Formats a number as currency with the locale's symbol, decimal separator, and symbol placement:

```typescript
{{#include ../../examples/i18n/snippets.ts:format-currency}}
```

```text
en: "Total: $23.10"
de: "Total: 23,10 €"
fr: "Total: 23,10 €"
ja: "Total: ¥23.10"
```

### Percent

Formats a decimal as a percentage (value is multiplied by 100):

```typescript
{{#include ../../examples/i18n/snippets.ts:format-percent}}
```

```text
en: "Discount: 15%"
de: "Discount: 15 %"
fr: "Discount: 15 %"
```

### FormatNumber

Formats a number with locale-appropriate grouping and decimal separators:

```typescript
{{#include ../../examples/i18n/snippets.ts:format-number}}
```

```text
en: "Population: 1,234,567.89"
de: "Population: 1.234.567,89"
fr: "Population: 1 234 567,89"
```

### ShortDate / LongDate

Formats a timestamp (milliseconds since epoch) as a date:

```typescript
{{#include ../../examples/i18n/snippets.ts:format-date}}
```

```text
ShortDate
  en: "Due: 3/22/2026"
  de: "Due: 22.03.2026"
  ja: "Due: 2026/03/22"

LongDate
  en: "Event: March 22, 2026"
  de: "Event: 22. März 2026"
  fr: "Event: 22. mars 2026"
```

### FormatTime

Formats a timestamp as time (12h vs 24h based on locale):

```typescript
{{#include ../../examples/i18n/snippets.ts:format-time}}
```

```text
en: "At: 3:45 PM"
de: "At: 15:45"
fr: "At: 15:45"
```

### Raw

Pass-through — prevents any automatic formatting. Use when a parameter name might trigger auto-formatting but you want the raw value:

```typescript
{{#include ../../examples/i18n/snippets.ts:format-raw}}
```

All locales: `"Code: 12345"` (no currency formatting despite the name).

## Locale-Specific Formatting Rules

Perry includes hand-rolled formatting rules for 25+ locales:

| Feature | Example Locales |
|---------|----------------|
| Decimal: `.` / Thousands: `,` | en, ja, zh, ko |
| Decimal: `,` / Thousands: `.` | de, nl, tr, es, it, pt |
| Decimal: `,` / Thousands: ` ` (narrow space) | fr |
| Decimal: `,` / Thousands: ` ` (non-breaking space) | ru, uk, pl, sv, da, no, fi |
| Currency before number: `$23.10` | en, ja, zh, ko |
| Currency after number: `23,10 €` | de, fr, es, it, ru |
| Percent with space: `42 %` | de, fr, es, ru |
| Percent without space: `42%` | en, ja, zh |
| Date order: M/D/Y | en |
| Date order: D.M.Y | de, fr, es, ru |
| Date order: Y/M/D | ja, zh, ko, sv |
| 24-hour time | de, fr, es, ja, zh, ru (most) |
| 12-hour time (AM/PM) | en |

## Currency Configuration

Configure default currency codes per locale in `perry.toml`:

```toml
[i18n]
locales = ["en", "de", "fr"]
default_locale = "en"

[i18n.currencies]
en = "USD"
de = "EUR"
fr = "EUR"
```

When `Currency(value)` is called, the active locale's configured ISO 4217 code determines the **symbol** (`USD` → `$`, `EUR` → `€`, `GBP` → `£`, …; codes without a well-known symbol render as the code itself), while the locale keeps controlling the **layout** — separators and symbol placement. With `en = "GBP"`, English output becomes `£1,234.50`; with `de = "USD"`, German output becomes `1.234,50 $`. Locales without an entry (exact locale first, then base language) use their built-in default symbol.
